# jsonschemalint

Validate a JSON file against a JSON Schema and get back the **exact path of
every field that broke** — `$.users[2].email: expected string, got number` —
not a pass/fail boolean you then have to bisect by hand. The only widely used
CLI for this is `ajv-cli`, which means installing Node and 26 npm packages to
check one file, and which reports a missing-property or extra-property error
against the *parent* object with the actual key buried in a `params` struct.

## Installation

```sh
cargo install --path .
```

## Usage

```bash
jsonschemalint schema.json data.json
```

With this schema (a small deployment manifest — `$ref` into `$defs`, nested
array of objects, a closed object):

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "required": ["service", "users"],
  "properties": {
    "service":  { "type": "string", "minLength": 1 },
    "replicas": { "type": "integer", "minimum": 1, "maximum": 10 },
    "users":    { "type": "array", "minItems": 1, "items": { "$ref": "#/$defs/user" } }
  },
  "additionalProperties": false,
  "$defs": {
    "user": {
      "type": "object",
      "required": ["name", "email", "role"],
      "properties": {
        "name":  { "type": "string" },
        "email": { "type": "string", "pattern": "^[^@]+@[^@]+$" },
        "role":  { "enum": ["admin", "editor", "viewer"] }
      },
      "additionalProperties": false
    }
  }
}
```

and this data:

```json
{
  "service": "billing",
  "replicas": 12,
  "users": [
    { "name": "ana", "email": "ana@example.com", "role": "admin"  },
    { "name": "bo",  "email": "bo@example.com",  "role": "editor" },
    { "name": "cy",  "email": 42, "role": "owner", "note": "temp" }
  ],
  "regoin": "eu-west-1"
}
```

you get:

```
$ jsonschemalint schema.json data.json
$.replicas: 12 is greater than the maximum 10
$.users[2].email: expected string, got number
$.users[2].role: "owner" is not one of the allowed values ["admin", "editor", "viewer"]
$.users[2].note: property "note" is not allowed here (additionalProperties is false)
$.regoin: property "regoin" is not allowed here (additionalProperties is false)
5 errors
```

Every line is a path you can paste straight into `jq`. The path survives the
`$ref` hop: `users[2]` is validated through `#/$defs/user`, but the error is
located in the *data* (`$.users[2].email`), never in the schema. Paths use
`a.b` for object keys, `a[2]` for array indices, and fall back to
`$["content-type"]` for any key that isn't a plain identifier, so a key with a
dot in it can never be mistaken for two path segments.

Exit `0` valid, `1` invalid, `2` couldn't run at all (missing file, malformed
JSON, `--strict` refusal) — so a script can tell "your data is wrong" apart
from "I never checked." `-q` suppresses all output and leaves just the exit
code; `-` in place of the data file reads the instance from stdin.

Errors are reported in the order fields appear in *your* file, and all of them
are reported. `ajv-cli` defaults to stopping at the first one unless you
remember `--all-errors`.

## What it validates

Supported, and each one produces a correctly pathed error when violated:
`type` (including `integer` accepting `1.0`, and the array form
`["string","null"]`), `required`, `properties`, `patternProperties`,
`additionalProperties`, `prefixItems`, `items`, `enum`, `const`, `minimum`,
`maximum`, `exclusiveMinimum`, `exclusiveMaximum`, `minLength`, `maxLength`,
`pattern`, `minItems`, `maxItems`, `uniqueItems`, `oneOf`, `anyOf`, `allOf`,
`not`, boolean schemas (`true`/`false` used where a schema is expected), and
`$ref` to a JSON Pointer within the same document — `#`, `#/$defs/User`,
`#/definitions/User`, `#/properties/foo`, including `~0`/`~1`/percent escapes
and recursive self-reference.

`patternProperties` and `prefixItems` are in that list because without them
`additionalProperties` and `items` are not actually Draft 2020-12 —
`additionalProperties` means "keys matched by neither `properties` nor
`patternProperties`", and `items` means "elements after the ones `prefixItems`
covered."

**Out of scope, stated plainly**:

- **Remote and `$id`-scoped `$ref`.** A `$ref` to another document
  (`https://…`, `common.json#/$defs/X`), a `$ref` resolved against an enclosing
  `$id`, and `$anchor`/`$dynamicRef` references are all rejected with an error,
  **not silently ignored**. Silently ignoring a `$ref` turns "invalid" into
  "valid", which is the one failure mode a validator must never have.
- **Unimplemented keywords**: `multipleOf`, `contains`/`minContains`/
  `maxContains`, `minProperties`/`maxProperties`, `dependentRequired`,
  `dependentSchemas`, `propertyNames`, `if`/`then`/`else`,
  `unevaluatedItems`/`unevaluatedProperties`, and the draft-07 `items`-as-array
  tuple form. These are **warned about on stderr before validating** —
  `warning: multipleOf at #/properties/count is not implemented and will be
  ignored` — because an assertion that gets skipped without saying so is
  indistinguishable from one that passed. `--strict` turns that warning into a
  refusal (exit `2`) so CI can't be fooled by a gap.
- **`format`** (`"format": "email"`) is not asserted. That is the *correct*
  default: in 2020-12 `format` is an annotation, not an assertion, so it gets
  no warning either.

## Status: built and verified against 561 real cases from the official JSON Schema Test Suite — all 561 pass

- **The official suite, vendored and run verbatim.** 22 files from
  [JSON-Schema-Test-Suite](https://github.com/json-schema-org/JSON-Schema-Test-Suite)
  `tests/draft2020-12/` (commit `ab079cc`, fetched 2026-09-19) are committed
  byte-for-byte under `tests/fixtures/draft2020-12/` — `type`, `required`,
  `properties`, `additionalProperties`, `items`, `enum`, `const`, `minimum`,
  `maximum`, `exclusiveMinimum`, `exclusiveMaximum`, `minLength`, `maxLength`,
  `pattern`, `minItems`, `maxItems`, `uniqueItems`, `oneOf`, `anyOf`, `allOf`,
  `not`, `ref`. That's **611 individual cases**, of which **561 are in scope
  and all 561 produce exactly the `valid` verdict the suite expects**. Nothing
  was edited to fit: this is independently authored ground truth, including
  the nasty ones — `const: -2.0` matching `-2`, `9007199254740992` *not*
  matching `9007199254740993`, `false` never equal to `0`, `1.0` counting as
  an `integer`, `minLength` counting code points so one emoji is one
  character, `pattern` being an unanchored search, and `$ref` pointers with
  `~0`/`~1`/`%25` escapes and empty tokens (`#/$defs//$defs/`). Tests read
  only the committed files — **no network access at test time**.
- **The 50 skipped cases are skipped on a rule, not because they failed.**
  They are 21 groups whose *schemas* use something listed as out of scope:
  `propertyNames`, `dependentSchemas`, `multipleOf`,
  `unevaluatedProperties`, or a remote/`$id`-scoped/`$anchor` `$ref`. Two
  separate tests enforce that this stays honest: one asserts every skip entry
  still matches a real group in the vendored files (no stale entries), and one
  asserts every skipped group's schema **demonstrably contains** an
  out-of-scope feature — so a group cannot be added to the skip list just
  because it fails. Run with the skip list disabled, exactly 25 cases in those
  21 groups disagree and **zero** cases outside them do; two of the 21 groups
  would actually pass anyway and are skipped only because the rule says so.
- **26 error-path tests**, which is the part the official suite does *not*
  cover — it asserts valid/invalid and never looks at where a validator says
  the problem is. One per supported keyword, each pinning the exact path
  string: `$.users[2].email` for a type error three levels down, `$[1][1]` for
  nested arrays, `$.db` + `missing required property "password"` for
  `required` (the missing key has no location of its own, so the error belongs
  to the object that should have held it), `$.typpo` for a rejected extra
  property, `$.xs[2]` + "already appears at index 0" for `uniqueItems`,
  `$.child.child.name` through a recursive `$ref: "#"`, and
  `$["a.b"]`/`$["content-type"]` for keys that must not be rendered as dot
  segments. Also pinned: that an unresolvable `$ref` and a `$ref` cycle each
  produce an error rather than a silent pass or a stack overflow.
- **8 CLI tests run the actual binary** against committed fixture files —
  including the exact schema/data pair printed in the Usage section above,
  asserting the full stdout byte-for-byte, so the README example cannot go
  stale. Exit codes `0`/`1`/`2`, `-q`, and the unsupported-keyword warning and
  `--strict` refusal are each covered.
- **Head-to-head against `ajv-cli` 5 on the identical schema and data.** Both
  found the same five violations and agreed on both verdicts (invalid, then
  valid after fixing). The differences are the reason this exists: `ajv-cli`
  needed `npm install` to pull **26 packages / 5.1 MB of `node_modules`** to
  check one file; it reported the stray `regoin` key as
  `instancePath: ''` + `params: { additionalProperty: 'regoin' }`, leaving you
  to join the two yourself, where `jsonschemalint` prints `$.regoin` directly;
  it printed the `enum` failure's allowed values as `allowedValues: [Array]`,
  so the one thing you needed — *what is allowed* — was literally elided by
  Node's console formatting, where `jsonschemalint` prints
  `["admin", "editor", "viewer"]`; and by default it stopped after the first
  error until told `--all-errors`.

**Not done / deliberately deferred**: multi-document schema bundles
(`$id` + remote `$ref`), which is a real v2 and needs a resolver and a
document store, not a bigger match statement. Inside `anyOf`/`oneOf` the
error stays at the branch point (`$.v: 3 does not match any of the 2 anyOf
subschemas`) rather than guessing which branch the author meant and reporting
that branch's inner errors — guessing produces confident nonsense, and
reporting every branch's errors produces noise. `allOf`, where all branches
must hold, does report its inner errors verbatim with their real paths.
