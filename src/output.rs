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

pub fn emit<W: std::io::Write>(
    w: &mut W,
    args: &IDLArgs,
    format: &OutputFormat,
    hash_map: &HashMap<u32, String>,
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
            writeln!(w, "{effective}")?;
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
        IDLValue::Vec(items) => IDLValue::Vec(items.iter().map(|v| resolve_value(v, map)).collect()),
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
