use anyhow::Result;
use candid::types::Label;
use candid::types::value::{IDLField, VariantValue};
use candid::{IDLArgs, IDLValue};
use std::collections::HashMap;

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

#[derive(Clone, Copy)]
pub struct FormatOpts {
    pub color: bool,
    pub compact: bool,
    pub blob_threshold: usize,
}

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";

fn paint(opts: &FormatOpts, code: &str, text: &str) -> String {
    if opts.color {
        format!("{}{}{}", code, text, RESET)
    } else {
        text.to_string()
    }
}

fn kw(opts: &FormatOpts, s: &str) -> String {
    paint(opts, BOLD, s)
}

fn type_ann(opts: &FormatOpts, t: &str) -> String {
    paint(opts, DIM, &format!(" : {}", t))
}

fn num_lit(opts: &FormatOpts, n: &str) -> String {
    paint(opts, YELLOW, n)
}

fn str_lit(opts: &FormatOpts, s: &str) -> String {
    // Rust's escape_debug matches the candid text escaping for common cases.
    let escaped = format!("{:?}", s);
    paint(opts, GREEN, &escaped)
}

fn principal_text(opts: &FormatOpts, p: &str) -> String {
    let quoted = format!("\"{}\"", p);
    paint(opts, MAGENTA, &quoted)
}

fn field_label(opts: &FormatOpts, label: &Label) -> String {
    let raw = match label {
        Label::Named(name) => ident_string(name),
        Label::Unnamed(idx) => idx.to_string(),
        // Label's Display uses pp_num_str for Id hashes
        Label::Id(hash) => pp_num_str(&hash.to_string()),
    };
    paint(opts, CYAN, &raw)
}

fn ident_string(name: &str) -> String {
    if name.chars().all(|c| c.is_alphanumeric() || c == '_')
        && name.chars().next().is_some_and(|c| !c.is_ascii_digit())
    {
        name.to_string()
    } else {
        format!("\"{}\"", name.escape_debug())
    }
}

// Thousands-separator format used by candid for Nat16/32/64 and Int16/32/64.
fn pp_num_str(s: &str) -> String {
    if let Some(stripped) = s.strip_prefix('-') {
        return format!("-{}", pp_num_str(stripped));
    }
    let bytes = s.as_bytes();
    let groups: std::vec::Vec<&str> = bytes
        .rchunks(3)
        .rev()
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    groups.join("_")
}

fn float_str(f: f64) -> String {
    if f.is_finite() && f.trunc() == f {
        format!("{f}.0")
    } else {
        f.to_string()
    }
}

fn has_type_annotation(v: &IDLValue) -> bool {
    use IDLValue::*;
    matches!(
        v,
        Int(_)
            | Nat(_)
            | Nat8(_)
            | Nat16(_)
            | Nat32(_)
            | Nat64(_)
            | Int8(_)
            | Int16(_)
            | Int32(_)
            | Int64(_)
            | Float32(_)
            | Float64(_)
            | Null
            | Reserved
    )
}

fn is_tuple_record(fields: &[IDLField]) -> bool {
    fields
        .iter()
        .enumerate()
        .all(|(i, f)| f.id.get_id() == i as u32)
}

fn format_blob_bytes(bytes: &[u8], opts: &FormatOpts) -> String {
    if bytes.len() > opts.blob_threshold {
        let hex_str = hex::encode(bytes);
        format!(
            "{}(\"{}\")",
            kw(opts, "blob_hex"),
            paint(opts, YELLOW, &hex_str)
        )
    } else {
        let is_all_ascii = bytes
            .iter()
            .all(|&b| (0x20..=0x7e).contains(&b) || matches!(b, 0x09 | 0x0a | 0x0d));
        let escaped: String = if is_all_ascii {
            bytes.iter().map(|&b| pp_char(b)).collect()
        } else {
            bytes.iter().map(|&b| format!("\\{:02x}", b)).collect()
        };
        format!("{} \"{}\"", kw(opts, "blob"), paint(opts, YELLOW, &escaped))
    }
}

fn pp_char(b: u8) -> String {
    let is_safe_ascii =
        (0x20..=0x7e).contains(&b) && b != 0x22 && b != 0x27 && b != 0x60 && b != 0x5c;
    if is_safe_ascii {
        (b as char).to_string()
    } else {
        format!("\\{:02x}", b)
    }
}

fn indent_str(n: usize) -> String {
    " ".repeat(n * 2)
}

pub fn format_value(v: &IDLValue, opts: &FormatOpts, depth: usize) -> String {
    use IDLValue::*;
    match v {
        Null => format!("{}{}", kw(opts, "null"), type_ann(opts, "null")),
        Bool(b) => kw(opts, if *b { "true" } else { "false" }),
        Number(n) => num_lit(opts, n),
        Int(n) => format!("{}{}", num_lit(opts, &n.to_string()), type_ann(opts, "int")),
        Nat(n) => format!("{}{}", num_lit(opts, &n.to_string()), type_ann(opts, "nat")),
        Nat8(n) => format!(
            "{}{}",
            num_lit(opts, &n.to_string()),
            type_ann(opts, "nat8")
        ),
        Nat16(n) => format!(
            "{}{}",
            num_lit(opts, &pp_num_str(&n.to_string())),
            type_ann(opts, "nat16")
        ),
        Nat32(n) => format!(
            "{}{}",
            num_lit(opts, &pp_num_str(&n.to_string())),
            type_ann(opts, "nat32")
        ),
        Nat64(n) => format!(
            "{}{}",
            num_lit(opts, &pp_num_str(&n.to_string())),
            type_ann(opts, "nat64")
        ),
        Int8(n) => format!(
            "{}{}",
            num_lit(opts, &n.to_string()),
            type_ann(opts, "int8")
        ),
        Int16(n) => format!(
            "{}{}",
            num_lit(opts, &pp_num_str(&n.to_string())),
            type_ann(opts, "int16")
        ),
        Int32(n) => format!(
            "{}{}",
            num_lit(opts, &pp_num_str(&n.to_string())),
            type_ann(opts, "int32")
        ),
        Int64(n) => format!(
            "{}{}",
            num_lit(opts, &pp_num_str(&n.to_string())),
            type_ann(opts, "int64")
        ),
        Float32(f) => format!(
            "{}{}",
            num_lit(opts, &float_str(*f as f64)),
            type_ann(opts, "float32")
        ),
        Float64(f) => format!(
            "{}{}",
            num_lit(opts, &float_str(*f)),
            type_ann(opts, "float64")
        ),
        Text(s) => str_lit(opts, s),
        None => kw(opts, "null"),
        Reserved => format!("{}{}", kw(opts, "null"), type_ann(opts, "reserved")),
        Principal(p) => {
            format!(
                "{} {}",
                kw(opts, "principal"),
                principal_text(opts, &p.to_text())
            )
        }
        Service(p) => format!("{} \"{}\"", kw(opts, "service"), p.to_text()),
        Func(p, m) => format!(
            "{} \"{}\".{}",
            kw(opts, "func"),
            p.to_text(),
            ident_string(m)
        ),
        Blob(bytes) => format_blob_bytes(bytes, opts),
        Opt(inner) => {
            let inner_str = format_value(inner, opts, depth);
            if has_type_annotation(inner) {
                format!("{} ({})", kw(opts, "opt"), inner_str)
            } else {
                format!("{} {}", kw(opts, "opt"), inner_str)
            }
        }
        Vec(items) => {
            // Vec<Nat8> is treated as blob
            if matches!(items.first(), Some(Nat8(_))) {
                let bytes: std::vec::Vec<u8> = items
                    .iter()
                    .map(|v| if let Nat8(b) = v { *b } else { 0 })
                    .collect();
                return format_blob_bytes(&bytes, opts);
            }
            format_vec(items, opts, depth)
        }
        Record(fields) => format_record(fields, opts, depth),
        Variant(VariantValue(field, _)) => format_variant(field, opts, depth),
    }
}

fn format_vec(items: &[IDLValue], opts: &FormatOpts, depth: usize) -> String {
    if items.is_empty() {
        return format!("{} {{}}", kw(opts, "vec"));
    }
    if opts.compact {
        let items_str = items
            .iter()
            .map(|v| format_value(v, opts, 0))
            .collect::<Vec<_>>()
            .join("; ");
        format!("{} {{ {} }}", kw(opts, "vec"), items_str)
    } else {
        let inner = indent_str(depth + 1);
        let close = indent_str(depth);
        let items_str = items
            .iter()
            .map(|v| format!("{}{};\n", inner, format_value(v, opts, depth + 1)))
            .collect::<String>();
        format!("{} {{\n{}{}}}", kw(opts, "vec"), items_str, close)
    }
}

fn format_record(fields: &[IDLField], opts: &FormatOpts, depth: usize) -> String {
    let tuple = is_tuple_record(fields);
    if fields.is_empty() {
        return format!("{} {{}}", kw(opts, "record"));
    }
    if opts.compact {
        let fields_str: Vec<String> = fields
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let val = format_value(&f.val, opts, 0);
                if tuple {
                    val
                } else if f.id.get_id() == i as u32 {
                    // unnamed positional field that happens to match index
                    val
                } else {
                    format!("{} = {}", field_label(opts, &f.id), val)
                }
            })
            .collect();
        format!("{} {{ {} }}", kw(opts, "record"), fields_str.join("; "))
    } else {
        let inner = indent_str(depth + 1);
        let close = indent_str(depth);
        let fields_str: String = fields
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let val = format_value(&f.val, opts, depth + 1);
                if tuple || f.id.get_id() == i as u32 {
                    format!("{}{};\n", inner, val)
                } else {
                    format!("{}{} = {};\n", inner, field_label(opts, &f.id), val)
                }
            })
            .collect();
        format!("{} {{\n{}{}}}", kw(opts, "record"), fields_str, close)
    }
}

fn format_variant(field: &IDLField, opts: &FormatOpts, depth: usize) -> String {
    let tag = field_label(opts, &field.id);
    let null_payload = matches!(field.val, IDLValue::Null);
    if opts.compact {
        if null_payload {
            format!("{} {{ {} }}", kw(opts, "variant"), tag)
        } else {
            let val = format_value(&field.val, opts, 0);
            format!("{} {{ {} = {} }}", kw(opts, "variant"), tag, val)
        }
    } else {
        let inner = indent_str(depth + 1);
        let close = indent_str(depth);
        if null_payload {
            format!("{} {{\n{}{};\n{}}}", kw(opts, "variant"), inner, tag, close)
        } else {
            let val = format_value(&field.val, opts, depth + 1);
            format!(
                "{} {{\n{}{} = {};\n{}}}",
                kw(opts, "variant"),
                inner,
                tag,
                val,
                close
            )
        }
    }
}

pub fn format_args(args: &IDLArgs, opts: &FormatOpts) -> String {
    let parts: Vec<String> = args.args.iter().map(|v| format_value(v, opts, 0)).collect();
    if opts.compact || parts.len() <= 1 {
        format!("({})", parts.join(", "))
    } else {
        let inner = indent_str(1);
        let items: String = parts
            .iter()
            .map(|p| format!("{}{}", inner, p))
            .collect::<Vec<_>>()
            .join(",\n");
        format!("(\n{}\n)", items)
    }
}

pub fn emit<W: std::io::Write>(
    w: &mut W,
    args: &IDLArgs,
    format: &OutputFormat,
    hash_map: &HashMap<u32, String>,
    opts: &FormatOpts,
) -> Result<()> {
    let resolved;
    let effective = if hash_map.is_empty() {
        args
    } else {
        resolved = resolve_args(args, hash_map);
        &resolved
    };
    match format {
        OutputFormat::Text => {
            if opts.color || opts.compact {
                // Use custom formatter (pretty+color or forced compact)
                writeln!(w, "{}", format_args(effective, opts))?;
            } else {
                // Legacy: candid Display handles line-wrapping at width 80
                writeln!(w, "{effective}")?;
            }
        }
        OutputFormat::Hex => {
            let bytes = effective.to_bytes()?;
            writeln!(w, "{}", hex::encode(&bytes))?;
        }
        OutputFormat::Bin => {
            let bytes = effective.to_bytes()?;
            w.write_all(&bytes)?;
        }
    }
    Ok(())
}

fn resolve_args(args: &IDLArgs, map: &HashMap<u32, String>) -> IDLArgs {
    IDLArgs {
        args: args.args.iter().map(|v| resolve_value(v, map)).collect(),
    }
}

fn resolve_value(v: &IDLValue, map: &HashMap<u32, String>) -> IDLValue {
    match v {
        IDLValue::Record(fields) => IDLValue::Record(
            fields
                .iter()
                .map(|f| IDLField {
                    id: resolve_label(&f.id, map),
                    val: resolve_value(&f.val, map),
                })
                .collect(),
        ),
        IDLValue::Variant(VariantValue(box_field, idx)) => IDLValue::Variant(VariantValue(
            Box::new(IDLField {
                id: resolve_label(&box_field.id, map),
                val: resolve_value(&box_field.val, map),
            }),
            *idx,
        )),
        IDLValue::Opt(inner) => IDLValue::Opt(Box::new(resolve_value(inner, map))),
        IDLValue::Vec(items) => {
            IDLValue::Vec(items.iter().map(|v| resolve_value(v, map)).collect())
        }
        other => other.clone(),
    }
}

fn resolve_label(label: &Label, map: &HashMap<u32, String>) -> Label {
    match label {
        Label::Id(h) => map
            .get(h)
            .map(|name| Label::Named(name.clone()))
            .unwrap_or(Label::Id(*h)),
        other => other.clone(),
    }
}
