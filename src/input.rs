use anyhow::{Context, Result};
use candid::IDLArgs;
use std::io::BufRead;

#[derive(Clone)]
pub enum InputFormat {
    Auto,
    Text, // also accepts "candid" as alias
    Hex,
    Bin,
}

impl InputFormat {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "candid" | "text" => Ok(InputFormat::Text),
            "hex" => Ok(InputFormat::Hex),
            "bin" => Ok(InputFormat::Bin),
            other => anyhow::bail!(
                "unknown input format: {other}; expected text (or candid), hex, or bin"
            ),
        }
    }
}

/// Peek at the first bytes and guess the format.
pub fn detect(peek: &[u8]) -> InputFormat {
    let trimmed: &[u8] = peek.iter()
        .position(|b| !b.is_ascii_whitespace())
        .map(|i| &peek[i..])
        .unwrap_or(&[]);
    if trimmed.starts_with(b"DIDL") {
        InputFormat::Bin
    } else if looks_like_hex(trimmed) {
        InputFormat::Hex
    } else {
        InputFormat::Text
    }
}

fn looks_like_hex(trimmed: &[u8]) -> bool {
    if trimmed.len() < 2 {
        return false;
    }
    trimmed
        .iter()
        .filter(|b| !b.is_ascii_whitespace())
        .all(|b| b.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------
// Streaming iterator
// ---------------------------------------------------------------------------

pub struct CandidStream<R: BufRead> {
    inner: StreamKind<R>,
}

enum StreamKind<R: BufRead> {
    Text(TextStream<R>),
    Hex(HexStream<R>),
    Bin(BinStream),
}

impl<R: BufRead> Iterator for CandidStream<R> {
    type Item = Result<IDLArgs>;
    fn next(&mut self) -> Option<Result<IDLArgs>> {
        match &mut self.inner {
            StreamKind::Text(s) => s.next(),
            StreamKind::Hex(s) => s.next(),
            StreamKind::Bin(s) => s.next(),
        }
    }
}

pub fn stream<R: BufRead>(mut reader: R, format: InputFormat) -> Result<CandidStream<R>> {
    let resolved = match format {
        InputFormat::Auto => {
            let peek = reader.fill_buf().context("failed to peek stdin")?;
            detect(peek)
        }
        other => other,
    };
    Ok(CandidStream {
        inner: match resolved {
            InputFormat::Text => StreamKind::Text(TextStream::new(reader)),
            InputFormat::Hex => StreamKind::Hex(HexStream::new(reader)),
            InputFormat::Bin => StreamKind::Bin(BinStream::new(reader)?),
            InputFormat::Auto => unreachable!(),
        },
    })
}

// ---------------------------------------------------------------------------
// Text stream: find complete top-level (…) groups
// ---------------------------------------------------------------------------

struct TextStream<R: BufRead> {
    reader: R,
    buf: String,
    eof: bool,
}

impl<R: BufRead> TextStream<R> {
    fn new(reader: R) -> Self {
        TextStream { reader, buf: String::new(), eof: false }
    }

    /// Drain one complete `(…)` group from `self.buf`, returning it.
    fn drain_one(&mut self) -> Option<String> {
        let bytes = self.buf.as_bytes();
        let mut depth: i32 = 0;
        let mut in_string = false;
        let mut escape = false;
        let mut group_start: Option<usize> = None;

        for (i, &b) in bytes.iter().enumerate() {
            if escape {
                escape = false;
                continue;
            }
            if in_string {
                match b {
                    b'\\' => escape = true,
                    b'"' => in_string = false,
                    _ => {}
                }
                continue;
            }
            match b {
                b'"' => in_string = true,
                b'(' => {
                    if depth == 0 {
                        group_start = Some(i);
                    }
                    depth += 1;
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(start) = group_start.take() {
                            let group = self.buf[start..=i].to_owned();
                            self.buf.drain(..=i);
                            return Some(group);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
}

impl<R: BufRead> Iterator for TextStream<R> {
    type Item = Result<IDLArgs>;

    fn next(&mut self) -> Option<Result<IDLArgs>> {
        loop {
            if let Some(group) = self.drain_one() {
                return Some(
                    candid_parser::parse_idl_args(&group)
                        .map_err(|e| anyhow::anyhow!("failed to parse Candid text: {e}")),
                );
            }
            if self.eof {
                // Check for non-whitespace leftovers
                if self.buf.trim().is_empty() {
                    return None;
                }
                return Some(Err(anyhow::anyhow!(
                    "failed to parse Candid text: incomplete expression at end of input"
                )));
            }
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Err(e) => return Some(Err(anyhow::anyhow!("failed to read stdin: {e}"))),
                Ok(0) => {
                    self.eof = true;
                    continue;
                }
                Ok(_) => self.buf.push_str(&line),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Hex stream: one frame per non-empty line
// ---------------------------------------------------------------------------

struct HexStream<R: BufRead> {
    reader: R,
}

impl<R: BufRead> HexStream<R> {
    fn new(reader: R) -> Self {
        HexStream { reader }
    }
}

impl<R: BufRead> Iterator for HexStream<R> {
    type Item = Result<IDLArgs>;

    fn next(&mut self) -> Option<Result<IDLArgs>> {
        loop {
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Err(e) => return Some(Err(anyhow::anyhow!("failed to read stdin: {e}"))),
                Ok(0) => return None,
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    return Some(
                        hex::decode(trimmed)
                            .with_context(|| {
                                if trimmed.len() % 2 != 0 {
                                    format!("invalid hex (odd length): {trimmed}")
                                } else {
                                    format!("invalid hex: {trimmed}")
                                }
                            })
                            .and_then(|raw| {
                                IDLArgs::from_bytes(&raw)
                                    .context("failed to decode binary Candid from hex")
                            }),
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Binary stream: parse-and-advance, one DIDL frame at a time
// ---------------------------------------------------------------------------

struct BinStream {
    buf: Vec<u8>,
}

impl BinStream {
    fn new<R: BufRead>(mut reader: R) -> Result<Self> {
        let mut buf = Vec::new();
        reader
            .read_to_end(&mut buf)
            .context("failed to read binary stdin")?;
        Ok(BinStream { buf })
    }
}

impl Iterator for BinStream {
    type Item = Result<IDLArgs>;

    fn next(&mut self) -> Option<Result<IDLArgs>> {
        if self.buf.is_empty() {
            return None;
        }
        let size = match crate::bin_frame::frame_size(&self.buf) {
            Ok(n) => n,
            Err(e) => return Some(Err(e)),
        };
        let result = IDLArgs::from_bytes(&self.buf[..size])
            .context("failed to decode binary Candid frame");
        self.buf.drain(..size);
        Some(result)
    }
}
