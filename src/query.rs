use anyhow::{bail, Result};
use candid::types::Label;
use candid::{IDLArgs, IDLValue};

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

enum Expr {
    Identity,
    Field(String),
    Pipe(Box<Expr>, Box<Expr>),
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

fn parse(s: &str) -> Result<Expr> {
    let (expr, rest) = parse_pipe(s.trim())?;
    if !rest.trim().is_empty() {
        bail!("unexpected characters in query: {:?}", rest.trim());
    }
    Ok(expr)
}

// pipe has lowest precedence: chain | chain | ...
fn parse_pipe(s: &str) -> Result<(Expr, &str)> {
    let (left, rest) = parse_chain(s)?;
    let rest = rest.trim_start();
    if let Some(after) = rest.strip_prefix('|') {
        let (right, rest2) = parse_pipe(after.trim_start())?;
        Ok((Expr::Pipe(Box::new(left), Box::new(right)), rest2))
    } else {
        Ok((left, rest))
    }
}

// chain: .foo.bar.baz is sugar for .foo | .bar | .baz
fn parse_chain(s: &str) -> Result<(Expr, &str)> {
    let s = s.trim_start();
    let after_dot = s
        .strip_prefix('.')
        .ok_or_else(|| anyhow::anyhow!("query must start with '.'"))?;

    let ident_end = after_dot
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(after_dot.len());
    let (ident, rest) = after_dot.split_at(ident_end);

    let atom = if ident.is_empty() {
        Expr::Identity
    } else {
        Expr::Field(ident.to_string())
    };

    // Chained dot: .foo.bar — only when next char after '.' is an identifier char
    let rest_trimmed = rest.trim_start();
    if rest_trimmed.starts_with('.')
        && rest_trimmed[1..]
            .starts_with(|c: char| c.is_alphabetic() || c == '_')
    {
        let (right, rest2) = parse_chain(rest_trimmed)?;
        Ok((Expr::Pipe(Box::new(atom), Box::new(right)), rest2))
    } else {
        Ok((atom, rest))
    }
}

// ---------------------------------------------------------------------------
// Evaluator
// ---------------------------------------------------------------------------

pub fn evaluate(args: IDLArgs, expr: Option<&str>) -> Result<Vec<IDLArgs>> {
    let expr_str = match expr {
        None | Some("") => return Ok(vec![args]),
        Some(s) => s.trim(),
    };
    if expr_str == "." {
        return Ok(vec![args]);
    }
    let query = parse(expr_str)?;
    eval_expr(&query, args)
}

fn eval_expr(expr: &Expr, args: IDLArgs) -> Result<Vec<IDLArgs>> {
    match expr {
        Expr::Identity => Ok(vec![args]),
        Expr::Field(name) => {
            let val = extract_field(&args, name)?;
            Ok(vec![IDLArgs::new(&[val])])
        }
        Expr::Pipe(left, right) => {
            let mut results = Vec::new();
            for item in eval_expr(left, args)? {
                results.extend(eval_expr(right, item)?);
            }
            Ok(results)
        }
    }
}

fn extract_field(args: &IDLArgs, name: &str) -> Result<IDLValue> {
    if args.args.len() != 1 {
        bail!(
            "field access '.{name}' requires a single value, got {} values",
            args.args.len()
        );
    }
    match &args.args[0] {
        IDLValue::Record(fields) => {
            let hash = candid::idl_hash(name);
            for field in fields {
                if label_matches(&field.id, name, hash) {
                    return Ok(field.val.clone());
                }
            }
            let available: Vec<String> = fields.iter().map(|f| label_display(&f.id)).collect();
            bail!(
                "unknown field '{name}'; available fields: {}",
                available.join(", ")
            );
        }
        other => bail!(
            "field access '.{name}' requires a record, got {}",
            type_name(other)
        ),
    }
}

fn label_matches(label: &Label, name: &str, hash: u32) -> bool {
    match label {
        Label::Named(n) => n == name,
        Label::Id(n) | Label::Unnamed(n) => *n == hash,
    }
}

fn label_display(label: &Label) -> String {
    match label {
        Label::Named(n) => n.clone(),
        Label::Id(n) | Label::Unnamed(n) => n.to_string(),
    }
}

fn type_name(val: &IDLValue) -> &'static str {
    match val {
        IDLValue::Bool(_) => "bool",
        IDLValue::Null | IDLValue::None => "null",
        IDLValue::Text(_) => "text",
        IDLValue::Number(_) => "number",
        IDLValue::Float64(_) | IDLValue::Float32(_) => "float",
        IDLValue::Opt(_) => "opt",
        IDLValue::Vec(_) => "vec",
        IDLValue::Record(_) => "record",
        IDLValue::Variant(_) => "variant",
        IDLValue::Blob(_) => "blob",
        IDLValue::Principal(_) => "principal",
        IDLValue::Service(_) => "service",
        IDLValue::Func(_, _) => "func",
        IDLValue::Int(_) => "int",
        IDLValue::Nat(_) => "nat",
        IDLValue::Nat8(_) => "nat8",
        IDLValue::Nat16(_) => "nat16",
        IDLValue::Nat32(_) => "nat32",
        IDLValue::Nat64(_) => "nat64",
        IDLValue::Int8(_) => "int8",
        IDLValue::Int16(_) => "int16",
        IDLValue::Int32(_) => "int32",
        IDLValue::Int64(_) => "int64",
        IDLValue::Reserved => "reserved",
    }
}
