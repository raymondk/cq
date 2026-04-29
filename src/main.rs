mod bin_frame;
mod cli;
mod input;
mod output;
mod query;
mod schema;

use anyhow::Result;
use clap::Parser;
use std::io::IsTerminal;

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

    let is_tty = std::io::stdout().is_terminal();
    let no_color = std::env::var("NO_COLOR").is_ok();

    let use_color = match args.color {
        cli::ColorMode::Always => true,
        cli::ColorMode::Never => false,
        cli::ColorMode::Auto => is_tty && !no_color,
    };

    let compact = args.compact;

    let fmt_opts = output::FormatOpts {
        color: use_color,
        compact,
        blob_threshold: args.blob_threshold,
    };

    let stdin = std::io::stdin().lock();
    let reader = std::io::BufReader::new(stdin);
    let stream = input::stream(reader, input_format)?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let mut emitted = 0usize;
    for value_result in stream {
        let args_val = value_result?;
        let results = query::evaluate(args_val, args.query.as_deref())?;
        for result in results {
            output::emit(
                &mut out,
                &result,
                &output_format,
                &schema.hash_to_name,
                &fmt_opts,
            )?;
            emitted += 1;
        }
    }

    if args.exit_status && emitted == 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("cq: {e}");
        std::process::exit(1);
    }
}
