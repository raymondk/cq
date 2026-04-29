use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "cq", about = "jq for Candid")]
pub struct Args {
    /// Query expression (default: identity)
    pub query: Option<String>,

    /// Input format: candid, hex, bin (default: auto)
    #[arg(long, value_name = "FORMAT")]
    pub input_format: Option<String>,

    /// Output format: candid, hex, bin (default: candid)
    #[arg(long, value_name = "FORMAT")]
    pub output: Option<String>,

    /// Path to .did schema file
    #[arg(long, value_name = "PATH")]
    pub did: Option<std::path::PathBuf>,
}
