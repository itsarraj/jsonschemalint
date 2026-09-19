//! The validator: a from-scratch implementation of the everyday subset of
//! JSON Schema Draft 2020-12, written so that every failure carries the exact
//! path of the value that failed.

use regex::Regex;
use serde_json::{Map, Value};

use crate::error::{child_index, child_key, ValidationError, ROOT};
use crate::pointer;
use crate::value::{is_type, json_eq, quote, render, render_list, type_name};

/// How many `$ref` hops deep we are willing to go before declaring a cycle.
///
/// Only `$ref` counts toward this: descending into `properties` or `items`
/// always consumes part of the instance and therefore terminates on its own.
/// A genuine `$ref` loop (`#/$defs/a` -> `#/$defs/a`) makes no progress at
/// all, so a hop counter is what catches it.
const MAX_REF_DEPTH: usize = 256;

/// Validates `instance` against `schema`, returning every failure found.
///
/// An empty vector means the instance is valid. Errors are returned in
/// document order, parents before children.
pub fn validate(schema: &Value, instance: &Value) -> Vec<ValidationError> {
    Validator::new(schema).validate(instance)
}

/// A schema, prepared for validating many instances against it.
///
/// Holding the schema document is what makes local `$ref` work: every
/// `#/$defs/...` pointer is resolved against this root, no matter how deep in
/// the schema the reference appears.
pub struct Validator<'a> {
    root: &'a Value,
}

impl<'a> Validator<'a> {
    pub fn new(schema: &'a Value) -> Self {
        Validator { root: schema }
    }

    /// Every validation failure in `instance`, in document order.
    pub fn validate(&self, instance: &Value) -> Vec<ValidationError> {
        let mut errors = Vec::new();
        self.check(self.root, instance, ROOT, 0, &mut errors);
        errors
    }

    /// Whether `instance` is valid, without building the error list.
    pub fn is_valid(&self, instance: &Value) -> bool {
        self.validate(instance).is_empty()
    }

    fn check(
        &self,
        schema: &Value,
        instance: &Value,
        path: &str,
        depth: usize,
        errors: &mut Vec<ValidationError>,
    ) {
        let map = match schema {
            // A boolean schema is a whole schema: `true` accepts anything,
            // `false` accepts nothing.
            Value::Bool(true) => return,
            Value::Bool(false) => {
                errors.push(ValidationError::new(
                    path,
                    "no value is allowed here (the schema for this location is `false`)",
                ));
                return;
            }
            Value::Object(map) => map,
            other => {
                errors.push(ValidationError::new(
                    path,
                    format!(
                        "invalid schema: expected an object or boolean, got {}",
                        type_name(other)
                    ),
                ));
                return;
            }
        };

        // In 2020-12 `$ref` does not replace its siblings: both apply.
        self.check_ref(map, instance, path, depth, errors);
        self.check_type(map, instance, path, errors);
        self.check_const_and_enum(map, instance, path, errors);
        if instance.is_number() {
            check_number(map, instance, path, errors);
        }
        if let Some(text) = instance.as_str() {
            check_string(map, text, instance, path, errors);
        }
        if let Some(items) = instance.as_array() {
            self.check_array(map, items, path, depth, errors);
        }
        if let Some(obj) = instance.as_object() {
            self.check_object(map, obj, path, depth, errors);
        }
        self.check_combinators(map, instance, path, depth, errors);
    }

    /// Runs a subschema purely to ask "does this match?", discarding the
    /// errors. Used by `anyOf`/`oneOf`/`not`, where a failing branch is not
    /// itself a problem.
    fn matches(&self, schema: &Value, instance: &Value, path: &str, depth: usize) -> bool {
        let mut scratch = Vec::new();
        self.check(schema, instance, path, depth, &mut scratch);
        scratch.is_empty()
    }

    fn check_ref(
        &self,
        map: &Map<String, Value>,
        instance: &Value,
        path: &str,
        depth: usize,
        errors: &mut Vec<ValidationError>,
    ) {
        let Some(reference) = map.get("$ref") else {
            return;
        };
        if depth >= MAX_REF_DEPTH {
            errors.push(ValidationError::new(
                path,
                format!("$ref cycle: followed {MAX_REF_DEPTH} references without terminating"),
            ));
            return;
        }
        match reference.as_str() {
            Some(text) => match pointer::resolve(self.root, text) {
                Ok(target) => self.check(target, instance, path, depth + 1, errors),
                Err(err) => errors.push(ValidationError::new(path, err.to_string())),
            },
            None => errors.push(ValidationError::new(
                path,
                format!(
                    "invalid schema: $ref must be a string, got {}",
                    type_name(reference)
                ),
            )),
        }
    }

    fn check_type(
        &self,
        map: &Map<String, Value>,
        instance: &Value,
        path: &str,
        errors: &mut Vec<ValidationError>,
    ) {
        let Some(spec) = map.get("type") else {
            return;
        };
        // `type` is either one name or a list of acceptable ones.
        let allowed: Vec<&str> = match spec {
            Value::String(name) => vec![name.as_str()],
            Value::Array(names) => names.iter().filter_map(|n| n.as_str()).collect(),
            other => {
                errors.push(ValidationError::new(
                    path,
                    format!(
                        "invalid schema: type must be a string or array of strings, got {}",
                        type_name(other)
                    ),
                ));
                return;
            }
        };
        if allowed.iter().any(|name| is_type(instance, name)) {
            return;
        }
        let expected = match allowed.len() {
            1 => allowed[0].to_string(),
            _ => format!("one of [{}]", allowed.join(", ")),
        };
        errors.push(ValidationError::new(
            path,
            format!("expected {expected}, got {}", type_name(instance)),
        ));
    }

    fn check_const_and_enum(
        &self,
        map: &Map<String, Value>,
        instance: &Value,
        path: &str,
        errors: &mut Vec<ValidationError>,
    ) {
        if let Some(expected) = map.get("const") {
            if !json_eq(instance, expected) {
                errors.push(ValidationError::new(
                    path,
                    format!(
                        "expected the constant {}, got {}",
                        render(expected),
                        render(instance)
                    ),
                ));
            }
        }
        let Some(spec) = map.get("enum") else {
            return;
        };
        let Some(choices) = spec.as_array() else {
            errors.push(ValidationError::new(
                path,
                format!(
                    "invalid schema: enum must be an array, got {}",
                    type_name(spec)
                ),
            ));
            return;
        };
        if !choices.iter().any(|c| json_eq(instance, c)) {
            errors.push(ValidationError::new(
                path,
                format!(
                    "{} is not one of the allowed values {}",
                    render(instance),
                    render_list(choices)
                ),
            ));
        }
    }

    fn check_array(
        &self,
        map: &Map<String, Value>,
        items: &[Value],
        path: &str,
        depth: usize,
        errors: &mut Vec<ValidationError>,
    ) {
        if let Some(limit) = map.get("minItems") {
            if let Some(min) = limit.as_f64() {
                if (items.len() as f64) < min {
                    errors.push(ValidationError::new(
                        path,
                        format!(
                            "array has {} items, fewer than the minimum {}",
                            items.len(),
                            render(limit)
                        ),
                    ));
                }
            }
        }
        if let Some(limit) = map.get("maxItems") {
            if let Some(max) = limit.as_f64() {
                if (items.len() as f64) > max {
                    errors.push(ValidationError::new(
                        path,
                        format!(
                            "array has {} items, more than the maximum {}",
                            items.len(),
                            render(limit)
                        ),
                    ));
                }
            }
        }
        if map.get("uniqueItems") == Some(&Value::Bool(true)) {
            for i in 1..items.len() {
                if let Some(first) = (0..i).find(|&j| json_eq(&items[j], &items[i])) {
                    errors.push(ValidationError::new(
                        child_index(path, i),
                        format!(
                            "duplicate item: {} already appears at index {first} \
                             (uniqueItems is true)",
                            render(&items[i])
                        ),
                    ));
                }
            }
        }

        // `prefixItems` positionally schemas the head of the array; `items`
        // then covers everything left over. Only a *sibling* `prefixItems`
        // counts -- one buried inside an `allOf` does not shift the index
        // `items` starts at.
        let mut prefix_len = 0;
        match map.get("prefixItems") {
            None => {}
            Some(Value::Array(schemas)) => {
                prefix_len = schemas.len();
                for (i, sub) in schemas.iter().enumerate() {
                    if let Some(item) = items.get(i) {
                        self.check(sub, item, &child_index(path, i), depth, errors);
                    }
                }
            }
            Some(other) => errors.push(ValidationError::new(
                path,
                format!(
                    "invalid schema: prefixItems must be an array of schemas, got {}",
                    type_name(other)
                ),
            )),
        }
        if let Some(sub) = map.get("items") {
            for (i, item) in items.iter().enumerate().skip(prefix_len) {
                let child = child_index(path, i);
                if sub == &Value::Bool(false) {
                    errors.push(ValidationError::new(
                        child,
                        format!(
                            "unexpected array item: only {prefix_len} item(s) are allowed here \
                             (items is false)"
                        ),
                    ));
                } else {
                    self.check(sub, item, &child, depth, errors);
                }
            }
        }
    }

    fn check_object(
        &self,
        map: &Map<String, Value>,
        obj: &Map<String, Value>,
        path: &str,
        depth: usize,
        errors: &mut Vec<ValidationError>,
    ) {
        match map.get("required") {
            None => {}
            Some(Value::Array(names)) => {
                for name in names {
                    if let Some(name) = name.as_str() {
                        if !obj.contains_key(name) {
                            errors.push(ValidationError::new(
                                path,
                                format!("missing required property {}", quote(name)),
                            ));
                        }
                    }
                }
            }
            Some(other) => errors.push(ValidationError::new(
                path,
                format!(
                    "invalid schema: required must be an array of strings, got {}",
                    type_name(other)
                ),
            )),
        }

        let properties = map.get("properties").and_then(Value::as_object);
        let mut patterns: Vec<(Regex, &Value)> = Vec::new();
        if let Some(pattern_properties) = map.get("patternProperties").and_then(Value::as_object) {
            for (source, sub) in pattern_properties {
                match Regex::new(source) {
                    Ok(re) => patterns.push((re, sub)),
                    Err(_) => errors.push(ValidationError::new(
                        path,
                        format!(
                            "invalid schema: patternProperties key {} is not a valid \
                             regular expression",
                            quote(source)
                        ),
                    )),
                }
            }
        }
        let additional = map.get("additionalProperties");

        // One pass over the *instance's* keys, so errors come out in the order
        // the fields appear in the user's document rather than in schema order.
        for (key, value) in obj {
            let child = child_key(path, key);
            let mut covered = false;
            if let Some(sub) = properties.and_then(|p| p.get(key)) {
                covered = true;
                self.check(sub, value, &child, depth, errors);
            }
            for (re, sub) in &patterns {
                if re.is_match(key) {
                    covered = true;
                    self.check(sub, value, &child, depth, errors);
                }
            }
            if covered {
                continue;
            }
            match additional {
                None => {}
                Some(Value::Bool(false)) => errors.push(ValidationError::new(
                    child,
                    format!(
                        "property {} is not allowed here (additionalProperties is false)",
                        quote(key)
                    ),
                )),
                Some(sub) => self.check(sub, value, &child, depth, errors),
            }
        }
    }

    fn check_combinators(
        &self,
        map: &Map<String, Value>,
        instance: &Value,
        path: &str,
        depth: usize,
        errors: &mut Vec<ValidationError>,
    ) {
        // `allOf` is the one combinator whose inner errors are unambiguously
        // the user's problem, so they are reported verbatim with full paths.
        if let Some(subs) = subschema_list(map, "allOf", path, errors) {
            for sub in subs {
                self.check(sub, instance, path, depth, errors);
            }
        }
        if let Some(subs) = subschema_list(map, "anyOf", path, errors) {
            if !subs.iter().any(|s| self.matches(s, instance, path, depth)) {
                errors.push(ValidationError::new(
                    path,
                    format!(
                        "{} does not match any of the {} anyOf subschemas",
                        render(instance),
                        subs.len()
                    ),
                ));
            }
        }
        if let Some(subs) = subschema_list(map, "oneOf", path, errors) {
            let matched: Vec<usize> = subs
                .iter()
                .enumerate()
                .filter(|(_, s)| self.matches(s, instance, path, depth))
                .map(|(i, _)| i)
                .collect();
            if matched.is_empty() {
                errors.push(ValidationError::new(
                    path,
                    format!(
                        "{} does not match any of the {} oneOf subschemas",
                        render(instance),
                        subs.len()
                    ),
                ));
            } else if matched.len() > 1 {
                let indices: Vec<String> = matched.iter().map(usize::to_string).collect();
                errors.push(ValidationError::new(
                    path,
                    format!(
                        "{} matches {} of the {} oneOf subschemas (indices {}); exactly one \
                         must match",
                        render(instance),
                        matched.len(),
                        subs.len(),
                        indices.join(", ")
                    ),
                ));
            }
        }
        if let Some(sub) = map.get("not") {
            if self.matches(sub, instance, path, depth) {
                errors.push(ValidationError::new(
                    path,
                    format!("{} must not match the `not` subschema", render(instance)),
                ));
            }
        }
    }
}

fn subschema_list<'s>(
    map: &'s Map<String, Value>,
    keyword: &str,
    path: &str,
    errors: &mut Vec<ValidationError>,
) -> Option<&'s Vec<Value>> {
    match map.get(keyword) {
        None => None,
        Some(Value::Array(subs)) => Some(subs),
        Some(other) => {
            errors.push(ValidationError::new(
                path,
                format!(
                    "invalid schema: {keyword} must be an array of schemas, got {}",
                    type_name(other)
                ),
            ));
            None
        }
    }
}

fn check_number(
    map: &Map<String, Value>,
    instance: &Value,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    let Some(value) = instance.as_f64() else {
        return;
    };
    if let Some(limit) = map.get("minimum") {
        if let Some(min) = limit.as_f64() {
            if value < min {
                errors.push(ValidationError::new(
                    path,
                    format!(
                        "{} is less than the minimum {}",
                        render(instance),
                        render(limit)
                    ),
                ));
            }
        }
    }
    if let Some(limit) = map.get("maximum") {
        if let Some(max) = limit.as_f64() {
            if value > max {
                errors.push(ValidationError::new(
                    path,
                    format!(
                        "{} is greater than the maximum {}",
                        render(instance),
                        render(limit)
                    ),
                ));
            }
        }
    }
    if let Some(limit) = map.get("exclusiveMinimum") {
        if let Some(min) = limit.as_f64() {
            if value <= min {
                errors.push(ValidationError::new(
                    path,
                    format!(
                        "{} is not greater than the exclusive minimum {}",
                        render(instance),
                        render(limit)
                    ),
                ));
            }
        }
    }
    if let Some(limit) = map.get("exclusiveMaximum") {
        if let Some(max) = limit.as_f64() {
            if value >= max {
                errors.push(ValidationError::new(
                    path,
                    format!(
                        "{} is not less than the exclusive maximum {}",
                        render(instance),
                        render(limit)
                    ),
                ));
            }
        }
    }
}

fn check_string(
    map: &Map<String, Value>,
    text: &str,
    instance: &Value,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    // Lengths are counted in Unicode code points, not bytes: a single emoji is
    // one character to `minLength`, however many bytes it takes to store.
    let length = text.chars().count();
    if let Some(limit) = map.get("minLength") {
        if let Some(min) = limit.as_f64() {
            if (length as f64) < min {
                errors.push(ValidationError::new(
                    path,
                    format!(
                        "string is {length} character(s) long, shorter than the minimum \
                         length {}",
                        render(limit)
                    ),
                ));
            }
        }
    }
    if let Some(limit) = map.get("maxLength") {
        if let Some(max) = limit.as_f64() {
            if (length as f64) > max {
                errors.push(ValidationError::new(
                    path,
                    format!(
                        "string is {length} character(s) long, longer than the maximum \
                         length {}",
                        render(limit)
                    ),
                ));
            }
        }
    }
    if let Some(source) = map.get("pattern") {
        match source.as_str() {
            Some(source) => match Regex::new(source) {
                // JSON Schema `pattern` is an unanchored search, so `a+`
                // matches anywhere in the string unless the author wrote
                // `^`/`$` themselves.
                Ok(re) => {
                    if !re.is_match(text) {
                        errors.push(ValidationError::new(
                            path,
                            format!(
                                "{} does not match the pattern {}",
                                render(instance),
                                quote(source)
                            ),
                        ));
                    }
                }
                Err(_) => errors.push(ValidationError::new(
                    path,
                    format!(
                        "invalid schema: pattern {} is not a valid regular expression",
                        quote(source)
                    ),
                )),
            },
            None => errors.push(ValidationError::new(
                path,
                format!(
                    "invalid schema: pattern must be a string, got {}",
                    type_name(source)
                ),
            )),
        }
    }
}
