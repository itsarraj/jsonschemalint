use std::fs;
use std::io::{self, Read};
use std::process;

use anyhow::{Context, Result};
use clap::Parser;
use serde_json::Value;

use jsonschemalint::{unsupported_keywords, validate};

#[derive(Parser)]
#[command(
    name = "jsonschemalint",
    about = "Validate a JSON document against a JSON Schema and report the exact path of every error"
)]
struct Cli {
    /// Path to the JSON Schema document.
    schema: String,

    /// Path to the JSON instance document to validate. Use `-` for stdin.
    instance: String,

    /// Print nothing; communicate only through the exit code.
    #[arg(short, long)]
    quiet: bool,

    /// Treat a schema that uses an unsupported keyword as a hard failure
    /// instead of a warning, so a skipped assertion can never be mistaken
    /// for a passing one.
    #[arg(long)]
    strict: bool,
}

fn read_json(path: &str) -> Result<Value> {
    let text = if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .context("reading JSON from stdin")?;
        buf
    } else {
        fs::read_to_string(path).with_context(|| format!("reading {path}"))?
    };
    serde_json::from_str(&text).with_context(|| format!("parsing {path} as JSON"))
}

fn run(cli: &Cli) -> Result<i32> {
    let schema = read_json(&cli.schema)?;
    let instance = read_json(&cli.instance)?;

    let gaps = unsupported_keywords(&schema);
    if !gaps.is_empty() {
        if cli.strict {
            let list: Vec<String> = gaps
                .iter()
                .map(|g| format!("{} at {}", g.keyword, g.location))
                .collect();
            anyhow::bail!(
                "--strict: this schema uses {} keyword(s) jsonschemalint does not implement, \
                 so the result would be incomplete: {}",
                gaps.len(),
                list.join(", ")
            );
        }
        if !cli.quiet {
            for gap in &gaps {
                eprintln!(
                    "warning: {} at {} is not implemented and will be ignored",
                    gap.keyword, gap.location
                );
            }
        }
    }

    let errors = validate(&schema, &instance);
    if errors.is_empty() {
        if !cli.quiet {
            println!("valid");
        }
        return Ok(0);
    }
    if !cli.quiet {
        for error in &errors {
            println!("{error}");
        }
        let plural = if errors.len() == 1 { "" } else { "s" };
        println!("{} error{plural}", errors.len());
    }
    Ok(1)
}

fn main() {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => process::exit(code),
        // Exit 2 for "could not run at all" (missing file, malformed JSON,
        // --strict refusal), keeping exit 1 to mean exactly "the instance is
        // invalid" so scripts can tell the two apart.
        Err(err) => {
            eprintln!("jsonschemalint: {err:#}");
            process::exit(2);
        }
    }
}
