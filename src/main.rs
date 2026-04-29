mod cli;
mod input;
mod output;
mod query;
mod schema;

use anyhow::Result;
use clap::Parser;
use std::io::Read;

fn run() -> Result<()> {
    let args = cli::Args::parse();

    let input_format = match &args.input_format {
        Some(s) => input::InputFormat::from_str(s)?,
        None => input::InputFormat::Auto,
    };
    let output_format = match &args.output {
        Some(s) => output::OutputFormat::from_str(s)?,
        None => output::OutputFormat::Candid,
    };

    let _schema = schema::SchemaResolver::load(args.did.as_deref())?;

    let mut stdin_bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut stdin_bytes)
        .map_err(|e| anyhow::anyhow!("failed to read stdin: {e}"))?;

    let parsed = input::parse(&stdin_bytes, &input_format)?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for value in parsed {
        let results = query::evaluate(value, args.query.as_deref())?;
        for result in results {
            output::emit(&mut out, &result, &output_format)?;
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("cq: {e}");
        std::process::exit(1);
    }
}
