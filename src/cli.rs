use clap::Parser;

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

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

    /// Path to .did schema file (can be specified multiple times)
    #[arg(long, value_name = "PATH")]
    pub did: Vec<std::path::PathBuf>,

    /// Color output: auto, always, never (default: auto)
    #[arg(long, value_name = "WHEN", default_value = "auto")]
    pub color: ColorMode,

    /// Compact single-line output
    #[arg(short = 'c', long)]
    pub compact: bool,

    /// Blob rendering threshold in bytes; blobs longer than this use hex (default: 64)
    #[arg(long, value_name = "N", default_value = "64")]
    pub blob_threshold: usize,

    /// Exit with status 1 if no values were produced
    #[arg(short = 'e', long)]
    pub exit_status: bool,
}
