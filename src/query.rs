use anyhow::{bail, Result};
use candid::types::Label;
use candid::{IDLArgs, IDLValue};

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

enum Expr {
    Identity,
    /// `.foo` / `.Tag` — bool flag = optional (`.foo?` / `.Tag?`)
    Field(String, bool),
    Index(usize),
    Slice(Option<usize>, Option<usize>),
    Iter,
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

// chain: .foo.bar, .[0].foo — sugar for nested pipes
fn parse_chain(s: &str) -> Result<(Expr, &str)> {
    let s = s.trim_start();
    let after_dot = s
        .strip_prefix('.')
        .ok_or_else(|| anyhow::anyhow!("query must start with '.'"))?;

    let (atom, rest) = if after_dot.starts_with('[') {
        parse_bracket(after_dot)?
    } else {
        let ident_end = after_dot
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(after_dot.len());
        let (ident, rest) = after_dot.split_at(ident_end);
        if ident.is_empty() {
            (Expr::Identity, rest)
        } else {
            let (optional, rest) = if rest.starts_with('?') {
                (true, &rest[1..])
            } else {
                (false, rest)
            };
            (Expr::Field(ident.to_string(), optional), rest)
        }
    };

    chain_tail(atom, rest)
}

fn chain_tail(atom: Expr, rest: &str) -> Result<(Expr, &str)> {
    let rest_trimmed = rest.trim_start();
    if rest_trimmed.starts_with('.')
        && rest_trimmed[1..]
            .starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '[')
    {
        let (right, rest2) = parse_chain(rest_trimmed)?;
        Ok((Expr::Pipe(Box::new(atom), Box::new(right)), rest2))
    } else {
        Ok((atom, rest))
    }
}

// parse the bracket part: `[...]` — caller already consumed the leading `.`
fn parse_bracket(s: &str) -> Result<(Expr, &str)> {
    let inner = s.strip_prefix('[').unwrap().trim_start();

    if let Some(rest) = inner.strip_prefix(']') {
        return Ok((Expr::Iter, rest));
    }

    let (first_num, after_first) = parse_optional_usize(inner)?;
    let after_first = after_first.trim_start();

    if let Some(rest) = after_first.strip_prefix(']') {
        let n = first_num
            .ok_or_else(|| anyhow::anyhow!("expected index number inside '[]'"))?;
        return Ok((Expr::Index(n), rest));
    }

    if let Some(after_colon) = after_first.strip_prefix(':') {
        let (second_num, after_second) = parse_optional_usize(after_colon.trim_start())?;
        let after_second = after_second.trim_start();
        let rest = after_second
            .strip_prefix(']')
            .ok_or_else(|| anyhow::anyhow!("expected ']' to close slice expression"))?;
        return Ok((Expr::Slice(first_num, second_num), rest));
    }

    bail!("invalid bracket expression; expected ']' or ':'")
}

fn parse_optional_usize(s: &str) -> Result<(Option<usize>, &str)> {
    let num_end = s
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(s.len());
    if num_end == 0 {
        return Ok((None, s));
    }
    let n: usize = s[..num_end]
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid index: {:?}", &s[..num_end]))?;
    Ok((Some(n), &s[num_end..]))
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
        Expr::Field(name, optional) => {
            let vals = extract_field(&args, name, *optional)?;
            Ok(vals.into_iter().map(|v| IDLArgs::new(&[v])).collect())
        }
        Expr::Index(i) => {
            let val = extract_index(&args, *i)?;
            Ok(vec![IDLArgs::new(&[val])])
        }
        Expr::Slice(start, end) => {
            let val = extract_slice(&args, *start, *end)?;
            Ok(vec![IDLArgs::new(&[val])])
        }
        Expr::Iter => extract_iter(&args),
        Expr::Pipe(left, right) => {
            let mut results = Vec::new();
            for item in eval_expr(left, args)? {
                results.extend(eval_expr(right, item)?);
            }
            Ok(results)
        }
    }
}

fn extract_field(args: &IDLArgs, name: &str, optional: bool) -> Result<Vec<IDLValue>> {
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
                    return Ok(vec![field.val.clone()]);
                }
            }
            if optional {
                return Ok(vec![]);
            }
            let available: Vec<String> = fields.iter().map(|f| label_display(&f.id)).collect();
            bail!(
                "unknown field '{name}'; available fields: {}",
                available.join(", ")
            );
        }
        IDLValue::Variant(v) => {
            let field = &v.0;
            let hash = candid::idl_hash(name);
            if label_matches(&field.id, name, hash) {
                return Ok(vec![field.val.clone()]);
            }
            if optional {
                return Ok(vec![]);
            }
            let active = label_display(&field.id);
            bail!("tag mismatch: active tag is '{active}', tried to access '{name}'");
        }
        other => bail!(
            "field access '.{name}' requires a record or variant, got {}",
            type_name(other)
        ),
    }
}

fn extract_index(args: &IDLArgs, i: usize) -> Result<IDLValue> {
    if args.args.len() != 1 {
        bail!(
            "index access '.[{i}]' requires a single value, got {} values",
            args.args.len()
        );
    }
    match &args.args[0] {
        IDLValue::Vec(items) => items.get(i).cloned().ok_or_else(|| {
            anyhow::anyhow!(
                "index {i} out of bounds: vec has {} element(s)",
                items.len()
            )
        }),
        other => bail!(
            "index access '.[{i}]' requires a vec, got {}",
            type_name(other)
        ),
    }
}

fn extract_slice(args: &IDLArgs, start: Option<usize>, end: Option<usize>) -> Result<IDLValue> {
    if args.args.len() != 1 {
        bail!(
            "slice access requires a single value, got {} values",
            args.args.len()
        );
    }
    match &args.args[0] {
        IDLValue::Vec(items) => {
            let len = items.len();
            let s = start.unwrap_or(0);
            let e = end.unwrap_or(len).min(len);
            if s > len {
                bail!("slice start {s} out of bounds: vec has {len} element(s)");
            }
            Ok(IDLValue::Vec(items[s..e].to_vec()))
        }
        other => bail!("slice access requires a vec, got {}", type_name(other)),
    }
}

fn extract_iter(args: &IDLArgs) -> Result<Vec<IDLArgs>> {
    if args.args.len() != 1 {
        bail!(
            "iterator '.[]' requires a single value, got {} values",
            args.args.len()
        );
    }
    match &args.args[0] {
        IDLValue::Vec(items) => Ok(items
            .iter()
            .map(|v| IDLArgs::new(&[v.clone()]))
            .collect()),
        other => bail!(
            "iterator '.[]' requires a vec, got {}",
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
