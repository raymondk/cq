mod bin_frame;
mod cli;
mod input;
mod output;
mod query;
mod schema;

use anyhow::Result;
use clap::Parser;

fn run() -> Result<()> {
    let args = cli::Args::parse();

    let input_format = match &args.input_format {
        Some(s) => input::InputFormat::from_str(s)?,
        None => input::InputFormat::Auto,
    };
    let output_format = match &args.output {
        Some(s) => output::OutputFormat::from_str(s)?,
        None => output::OutputFormat::Text,
    };

    let did_refs: Vec<&std::path::Path> = args.did.iter().map(|p| p.as_path()).collect();
    let schema = schema::SchemaResolver::load(&did_refs)?;

    let stdin = std::io::stdin().lock();
    let reader = std::io::BufReader::new(stdin);
    let stream = input::stream(reader, input_format)?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for value_result in stream {
        let args_val = value_result?;
        let results = query::evaluate(args_val, args.query.as_deref())?;
        for result in results {
            output::emit(&mut out, &result, &output_format, &schema.hash_to_name)?;
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
