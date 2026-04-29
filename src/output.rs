use anyhow::Result;
use candid::IDLArgs;

pub enum OutputFormat {
    Text, // also accepts "candid" as alias
    Hex,
    Bin,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "text" | "candid" => Ok(OutputFormat::Text),
            "hex" => Ok(OutputFormat::Hex),
            "bin" => Ok(OutputFormat::Bin),
            other => anyhow::bail!(
                "unknown output format: {other}; expected text (or candid), hex, or bin"
            ),
        }
    }
}

pub fn emit<W: std::io::Write>(w: &mut W, args: &IDLArgs, format: &OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Text => {
            writeln!(w, "{args}")?;
        }
        OutputFormat::Hex => {
            let bytes = args.to_bytes()?;
            writeln!(w, "{}", hex::encode(&bytes))?;
        }
        OutputFormat::Bin => {
            let bytes = args.to_bytes()?;
            w.write_all(&bytes)?;
        }
    }
    Ok(())
}
