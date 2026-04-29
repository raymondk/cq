use anyhow::{Context, Result};
use candid::IDLArgs;

pub enum InputFormat {
    Auto,
    Candid,
    Hex,
    Bin,
}

impl InputFormat {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "candid" => Ok(InputFormat::Candid),
            "hex" => Ok(InputFormat::Hex),
            "bin" => Ok(InputFormat::Bin),
            other => anyhow::bail!("unknown input format: {other}; expected candid, hex, or bin"),
        }
    }
}

pub fn parse(bytes: &[u8], format: &InputFormat) -> Result<Vec<IDLArgs>> {
    match format {
        InputFormat::Candid => parse_text(bytes),
        InputFormat::Hex => parse_hex(bytes),
        InputFormat::Bin => parse_bin(bytes),
        InputFormat::Auto => {
            if looks_like_binary(bytes) {
                parse_bin(bytes)
            } else if looks_like_hex(bytes) {
                parse_hex(bytes)
            } else {
                parse_text(bytes)
            }
        }
    }
}

fn looks_like_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(b"DIDL")
}

fn looks_like_hex(bytes: &[u8]) -> bool {
    let trimmed = bytes.trim_ascii_start();
    if trimmed.is_empty() {
        return false;
    }
    trimmed
        .iter()
        .filter(|b| !b.is_ascii_whitespace())
        .all(|b| b.is_ascii_hexdigit())
        && trimmed.iter().filter(|b| !b.is_ascii_whitespace()).count() >= 2
}

fn parse_text(bytes: &[u8]) -> Result<Vec<IDLArgs>> {
    let s = std::str::from_utf8(bytes).context("input is not valid UTF-8")?;
    let s = s.trim();
    if s.is_empty() {
        return Ok(vec![]);
    }
    let args = candid_parser::parse_idl_args(s)
        .map_err(|e| anyhow::anyhow!("failed to parse Candid text: {e}"))?;
    Ok(vec![args])
}

fn parse_hex(bytes: &[u8]) -> Result<Vec<IDLArgs>> {
    let s = std::str::from_utf8(bytes).context("input is not valid UTF-8")?;
    let mut results = Vec::new();
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let raw = hex::decode(line)
            .with_context(|| format!("invalid hex on line: {line}"))?;
        let args = IDLArgs::from_bytes(&raw)
            .context("failed to decode binary Candid from hex")?;
        results.push(args);
    }
    Ok(results)
}

fn parse_bin(bytes: &[u8]) -> Result<Vec<IDLArgs>> {
    if bytes.is_empty() {
        return Ok(vec![]);
    }
    let args = IDLArgs::from_bytes(bytes).context("failed to decode binary Candid")?;
    Ok(vec![args])
}
