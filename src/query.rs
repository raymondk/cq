use anyhow::{bail, Result};
use candid::types::Label;
use candid::types::value::{IDLField, VariantValue};
use candid::{IDLArgs, IDLValue};

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum OptMode {
    Normal,   // .field
    Optional, // .field?  — empty on miss/None, unwrap if Some
    Assert,   // .field!  — error on miss/None, unwrap if Some
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TypeAscription {
    Nat,
    Int,
    Nat8,
    Nat16,
    Nat32,
    Nat64,
    Int8,
    Int16,
    Int32,
    Int64,
    Float32,
    Float64,
}

enum Expr {
    Identity,
    /// `.foo` / `.Tag` with an opt mode flag
    Field(String, OptMode),
    Index(usize),
    Slice(Option<usize>, Option<usize>),
    Iter,
    Pipe(Box<Expr>, Box<Expr>),
    /// `expr // fallback` — unwrap opt or use fallback
    Alt(Box<Expr>, Box<Expr>),
    /// `.?` — unwrap the current opt value (empty if None)
    OptUnwrap,
    /// `some(expr)` — wrap value in opt
    SomeOf(Box<Expr>),
    /// `none` — opt-None literal
    None_,
    /// `{a: expr, b: expr}` — record construction
    MakeRecord(Vec<(String, Box<Expr>)>),
    /// `[expr, expr, ...]` — vec construction
    MakeVec(Vec<Box<Expr>>),
    /// `variant { Tag = expr }` or `variant { Tag }`
    MakeVariant(String, Option<Box<Expr>>),
    /// `principal "text"`
    MakePrincipal(String),
    /// `blob "..."` — blob from candid escape string
    MakeBlob(Vec<u8>),
    /// `blob_hex(expr)` — blob from hex-producing expression
    BlobHex(Box<Expr>),
    /// `expr : Type` — type ascription
    Ascribe(Box<Expr>, TypeAscription),
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

// pipe has lowest precedence: ascribe | ascribe | ...
fn parse_pipe(s: &str) -> Result<(Expr, &str)> {
    let (left, rest) = parse_ascribe(s)?;
    let rest = rest.trim_start();
    if let Some(after) = rest.strip_prefix('|') {
        // Don't consume '//' as a pipe '|'
        if after.starts_with('/') {
            return Ok((left, rest));
        }
        let (right, rest2) = parse_pipe(after.trim_start())?;
        Ok((Expr::Pipe(Box::new(left), Box::new(right)), rest2))
    } else {
        Ok((left, rest))
    }
}

// ascribe: alt (: TypeName)?
fn parse_ascribe(s: &str) -> Result<(Expr, &str)> {
    let (expr, rest) = parse_alt(s)?;
    let rest_trimmed = rest.trim_start();
    if let Some(after_colon) = rest_trimmed.strip_prefix(':') {
        let after_colon = after_colon.trim_start();
        if let Some((ta, rest2)) = try_parse_type_name(after_colon) {
            return Ok((Expr::Ascribe(Box::new(expr), ta), rest2));
        }
    }
    Ok((expr, rest))
}

fn try_parse_type_name(s: &str) -> Option<(TypeAscription, &str)> {
    // Check longer keywords before shorter ones to avoid prefix matches
    let keywords: &[(&str, TypeAscription)] = &[
        ("nat8", TypeAscription::Nat8),
        ("nat16", TypeAscription::Nat16),
        ("nat32", TypeAscription::Nat32),
        ("nat64", TypeAscription::Nat64),
        ("int8", TypeAscription::Int8),
        ("int16", TypeAscription::Int16),
        ("int32", TypeAscription::Int32),
        ("int64", TypeAscription::Int64),
        ("float32", TypeAscription::Float32),
        ("float64", TypeAscription::Float64),
        ("nat", TypeAscription::Nat),
        ("int", TypeAscription::Int),
    ];
    for (kw, ta) in keywords {
        if let Some(rest) = s.strip_prefix(kw) {
            if rest
                .chars()
                .next()
                .map_or(true, |c| !c.is_alphanumeric() && c != '_')
            {
                return Some((*ta, rest));
            }
        }
    }
    None
}

// alt: atom // atom // ... — higher precedence than ascribe
fn parse_alt(s: &str) -> Result<(Expr, &str)> {
    let (left, rest) = parse_atom(s)?;
    let rest = rest.trim_start();
    if let Some(after) = rest.strip_prefix("//") {
        let (right, rest2) = parse_atom(after.trim_start())?;
        Ok((Expr::Alt(Box::new(left), Box::new(right)), rest2))
    } else {
        Ok((left, rest))
    }
}

// atom: dotchain | constructors | some(pipe) | none
fn parse_atom(s: &str) -> Result<(Expr, &str)> {
    let s = s.trim_start();

    // `none` keyword (not followed by alphanumeric or '_')
    if let Some(after) = s.strip_prefix("none") {
        if after
            .chars()
            .next()
            .map_or(true, |c| !c.is_alphanumeric() && c != '_')
        {
            return Ok((Expr::None_, after));
        }
    }

    // `some(pipe)`
    if let Some(after) = s.strip_prefix("some(") {
        let (inner, rest) = parse_pipe(after.trim_start())?;
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix(')')
            .ok_or_else(|| anyhow::anyhow!("expected ')' after some(...)"))?;
        return Ok((Expr::SomeOf(Box::new(inner)), rest));
    }

    // `principal "text"`
    if let Some(after) = s.strip_prefix("principal") {
        let after = after.trim_start();
        if after.starts_with('"') {
            let (text_bytes, rest) = parse_quoted_bytes(after)?;
            let text = String::from_utf8(text_bytes)
                .map_err(|_| anyhow::anyhow!("principal text must be valid UTF-8"))?;
            return Ok((Expr::MakePrincipal(text), rest));
        }
    }

    // `blob_hex(expr)` — must be checked before `blob "..."` to avoid prefix confusion
    if let Some(after) = s.strip_prefix("blob_hex(") {
        let (inner, rest) = parse_pipe(after.trim_start())?;
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix(')')
            .ok_or_else(|| anyhow::anyhow!("expected ')' after blob_hex(...)"))?;
        return Ok((Expr::BlobHex(Box::new(inner)), rest));
    }

    // `blob "..."`
    if let Some(after) = s.strip_prefix("blob") {
        let after = after.trim_start();
        if after.starts_with('"') {
            let (bytes, rest) = parse_quoted_bytes(after)?;
            return Ok((Expr::MakeBlob(bytes), rest));
        }
    }

    // `variant { Tag = expr }` or `variant { Tag }`
    if let Some(after) = s.strip_prefix("variant") {
        let after = after.trim_start();
        if after.starts_with('{') {
            let (name, payload, rest) = parse_variant_constructor(&after[1..])?;
            return Ok((Expr::MakeVariant(name, payload.map(Box::new)), rest));
        }
    }

    // `{ a: expr, b: expr }` — record construction
    if s.starts_with('{') {
        let (fields, rest) = parse_record_constructor(&s[1..])?;
        return Ok((Expr::MakeRecord(fields), rest));
    }

    // `[ expr, expr, ... ]` — vec construction
    if s.starts_with('[') {
        let (elems, rest) = parse_vec_constructor(&s[1..])?;
        return Ok((Expr::MakeVec(elems), rest));
    }

    // fallthrough to dotchain
    parse_dotchain(s)
}

// dotchain: .foo? .bar! .[0] etc. (chained via chain_tail)
fn parse_dotchain(s: &str) -> Result<(Expr, &str)> {
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
            if rest.starts_with('?') {
                (Expr::OptUnwrap, &rest[1..])
            } else {
                (Expr::Identity, rest)
            }
        } else {
            let (mode, rest) = if rest.starts_with('?') {
                (OptMode::Optional, &rest[1..])
            } else if rest.starts_with('!') {
                (OptMode::Assert, &rest[1..])
            } else {
                (OptMode::Normal, rest)
            };
            (Expr::Field(ident.to_string(), mode), rest)
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
        let (right, rest2) = parse_dotchain(rest_trimmed)?;
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

// Parse `{field: expr, field: expr}` — caller consumed the leading `{`
fn parse_record_constructor(s: &str) -> Result<(Vec<(String, Box<Expr>)>, &str)> {
    let mut fields = Vec::new();
    let mut rest = s.trim_start();
    if let Some(after) = rest.strip_prefix('}') {
        return Ok((fields, after));
    }
    loop {
        rest = rest.trim_start();
        let ident_end = rest
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        if ident_end == 0 {
            bail!("expected field name in record constructor, got {:?}", rest);
        }
        let name = rest[..ident_end].to_string();
        rest = rest[ident_end..].trim_start();
        rest = rest
            .strip_prefix(':')
            .ok_or_else(|| anyhow::anyhow!("expected ':' after field name '{name}'"))?;
        let (expr, after_expr) = parse_pipe(rest.trim_start())?;
        fields.push((name, Box::new(expr)));
        rest = after_expr.trim_start();
        if let Some(after) = rest.strip_prefix('}') {
            return Ok((fields, after));
        } else if let Some(after) = rest.strip_prefix(',') {
            rest = after;
        } else {
            bail!("expected ',' or '}}' in record constructor, got {:?}", rest);
        }
    }
}

// Parse `[expr, expr, ...]` — caller consumed the leading `[`
fn parse_vec_constructor(s: &str) -> Result<(Vec<Box<Expr>>, &str)> {
    let mut elems = Vec::new();
    let mut rest = s.trim_start();
    if let Some(after) = rest.strip_prefix(']') {
        return Ok((elems, after));
    }
    loop {
        let (expr, after_expr) = parse_pipe(rest.trim_start())?;
        elems.push(Box::new(expr));
        rest = after_expr.trim_start();
        if let Some(after) = rest.strip_prefix(']') {
            return Ok((elems, after));
        } else if let Some(after) = rest.strip_prefix(',') {
            rest = after;
        } else {
            bail!("expected ',' or ']' in vec constructor, got {:?}", rest);
        }
    }
}

// Parse `{ Tag = expr }` or `{ Tag }` — caller consumed the leading `{`
fn parse_variant_constructor(s: &str) -> Result<(String, Option<Expr>, &str)> {
    let mut rest = s.trim_start();
    let ident_end = rest
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    if ident_end == 0 {
        bail!("expected tag name in variant constructor, got {:?}", rest);
    }
    let name = rest[..ident_end].to_string();
    rest = rest[ident_end..].trim_start();

    let payload = if let Some(after_eq) = rest.strip_prefix('=') {
        let (expr, after_expr) = parse_pipe(after_eq.trim_start())?;
        rest = after_expr.trim_start();
        Some(expr)
    } else {
        None
    };

    rest = rest
        .strip_prefix('}')
        .ok_or_else(|| anyhow::anyhow!("expected '}}' to close variant constructor"))?;
    Ok((name, payload, rest))
}

// Parse a double-quoted string, returning raw bytes (handles \n \t \r \" \\ \XX hex escapes)
fn parse_quoted_bytes(s: &str) -> Result<(Vec<u8>, &str)> {
    let s = s
        .strip_prefix('"')
        .ok_or_else(|| anyhow::anyhow!("expected '\"'"))?;
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Ok((result, &s[i + 1..])),
            b'\\' => {
                i += 1;
                if i >= bytes.len() {
                    bail!("unterminated escape in string literal");
                }
                match bytes[i] {
                    b'n' => result.push(b'\n'),
                    b't' => result.push(b'\t'),
                    b'r' => result.push(b'\r'),
                    b'"' => result.push(b'"'),
                    b'\\' => result.push(b'\\'),
                    hi @ (b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F') => {
                        i += 1;
                        if i >= bytes.len() {
                            bail!("incomplete hex escape in string literal");
                        }
                        let lo = bytes[i];
                        result.push(hex_nibble(hi)? << 4 | hex_nibble(lo)?);
                    }
                    other => bail!("unknown string escape '\\{}'", other as char),
                }
            }
            b => result.push(b),
        }
        i += 1;
    }
    bail!("unterminated string literal")
}

fn hex_nibble(b: u8) -> Result<u8> {
    Ok(match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => bail!("invalid hex character: {:?}", b as char),
    })
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
        Expr::Field(name, mode) => {
            let vals = extract_field(&args, name, *mode)?;
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
        Expr::OptUnwrap => {
            if args.args.len() != 1 {
                bail!("'.?' requires a single value, got {}", args.args.len());
            }
            match &args.args[0] {
                IDLValue::Opt(inner) => Ok(vec![IDLArgs::new(&[*inner.clone()])]),
                IDLValue::None => Ok(vec![]),
                other => bail!("'.?' requires an opt value, got {}", type_name(other)),
            }
        }
        Expr::Alt(left, right) => {
            let results = eval_expr(left, args.clone())?;
            let unwrapped: Vec<IDLArgs> = results
                .into_iter()
                .filter_map(unwrap_opt_filter)
                .collect();
            if unwrapped.is_empty() {
                eval_expr(right, args)
            } else {
                Ok(unwrapped)
            }
        }
        Expr::SomeOf(inner) => {
            let results = eval_expr(inner, args)?;
            results
                .into_iter()
                .map(|r| {
                    if r.args.len() != 1 {
                        bail!("some() requires a single value, got {}", r.args.len());
                    }
                    Ok(IDLArgs::new(&[IDLValue::Opt(Box::new(r.args[0].clone()))]))
                })
                .collect()
        }
        Expr::None_ => Ok(vec![IDLArgs::new(&[IDLValue::None])]),

        Expr::MakeRecord(fields) => {
            let mut idl_fields = Vec::new();
            for (name, field_expr) in fields {
                let mut results = eval_expr(field_expr, args.clone())?;
                if results.len() != 1 || results[0].args.len() != 1 {
                    bail!(
                        "record field '{name}' must produce exactly one value, got {}",
                        results.len()
                    );
                }
                let val = results.remove(0).args.into_iter().next().unwrap();
                idl_fields.push(IDLField {
                    id: Label::Named(name.clone()),
                    val,
                });
            }
            Ok(vec![IDLArgs::new(&[IDLValue::Record(idl_fields)])])
        }

        Expr::MakeVec(elems) => {
            let mut items = Vec::new();
            for (i, elem_expr) in elems.iter().enumerate() {
                let mut results = eval_expr(elem_expr, args.clone())?;
                if results.len() != 1 || results[0].args.len() != 1 {
                    bail!(
                        "vec element {i} must produce exactly one value, got {}",
                        results.len()
                    );
                }
                items.push(results.remove(0).args.into_iter().next().unwrap());
            }
            Ok(vec![IDLArgs::new(&[IDLValue::Vec(items)])])
        }

        Expr::MakeVariant(name, payload_expr) => {
            let payload = if let Some(expr) = payload_expr {
                let mut results = eval_expr(expr, args)?;
                if results.len() != 1 || results[0].args.len() != 1 {
                    bail!("variant payload must produce exactly one value");
                }
                results.remove(0).args.into_iter().next().unwrap()
            } else {
                IDLValue::Null
            };
            let field = IDLField {
                id: Label::Named(name.clone()),
                val: payload,
            };
            Ok(vec![IDLArgs::new(&[IDLValue::Variant(VariantValue(
                Box::new(field),
                0,
            ))])])
        }

        Expr::MakePrincipal(text) => {
            let p = candid::Principal::from_text(text)
                .map_err(|e| anyhow::anyhow!("invalid principal {:?}: {e}", text))?;
            Ok(vec![IDLArgs::new(&[IDLValue::Principal(p)])])
        }

        Expr::MakeBlob(bytes) => Ok(vec![IDLArgs::new(&[IDLValue::Blob(bytes.clone())])]),

        Expr::BlobHex(inner) => {
            let mut results = eval_expr(inner, args)?;
            if results.len() != 1 || results[0].args.len() != 1 {
                bail!("blob_hex() requires exactly one text value");
            }
            let val = results.remove(0).args.into_iter().next().unwrap();
            match val {
                IDLValue::Text(s) => {
                    let bytes = hex::decode(&s)
                        .map_err(|e| anyhow::anyhow!("blob_hex: invalid hex string: {e}"))?;
                    Ok(vec![IDLArgs::new(&[IDLValue::Blob(bytes)])])
                }
                other => bail!(
                    "blob_hex() requires a text value, got {}",
                    type_name(&other)
                ),
            }
        }

        Expr::Ascribe(inner, target) => {
            let results = eval_expr(inner, args)?;
            results
                .into_iter()
                .map(|r| {
                    if r.args.len() != 1 {
                        bail!("type ascription requires a single value");
                    }
                    let val = apply_ascription(r.args.into_iter().next().unwrap(), *target)?;
                    Ok(IDLArgs::new(&[val]))
                })
                .collect()
        }
    }
}

fn apply_ascription(val: IDLValue, target: TypeAscription) -> Result<IDLValue> {
    let s = match &val {
        IDLValue::Number(s) => s.clone(),
        IDLValue::Nat8(n) => n.to_string(),
        IDLValue::Nat16(n) => n.to_string(),
        IDLValue::Nat32(n) => n.to_string(),
        IDLValue::Nat64(n) => n.to_string(),
        IDLValue::Int8(n) => n.to_string(),
        IDLValue::Int16(n) => n.to_string(),
        IDLValue::Int32(n) => n.to_string(),
        IDLValue::Int64(n) => n.to_string(),
        IDLValue::Nat(n) => n.to_string(),
        IDLValue::Int(n) => n.to_string(),
        IDLValue::Float32(f) => f.to_string(),
        IDLValue::Float64(f) => f.to_string(),
        other => bail!(
            "type ascription requires a numeric value, got {}",
            type_name(other)
        ),
    };

    match target {
        TypeAscription::Nat8 => s
            .parse::<u8>()
            .map(IDLValue::Nat8)
            .map_err(|_| anyhow::anyhow!("value {:?} is out of range for nat8", s)),
        TypeAscription::Nat16 => s
            .parse::<u16>()
            .map(IDLValue::Nat16)
            .map_err(|_| anyhow::anyhow!("value {:?} is out of range for nat16", s)),
        TypeAscription::Nat32 => s
            .parse::<u32>()
            .map(IDLValue::Nat32)
            .map_err(|_| anyhow::anyhow!("value {:?} is out of range for nat32", s)),
        TypeAscription::Nat64 => s
            .parse::<u64>()
            .map(IDLValue::Nat64)
            .map_err(|_| anyhow::anyhow!("value {:?} is out of range for nat64", s)),
        TypeAscription::Int8 => s
            .parse::<i8>()
            .map(IDLValue::Int8)
            .map_err(|_| anyhow::anyhow!("value {:?} is out of range for int8", s)),
        TypeAscription::Int16 => s
            .parse::<i16>()
            .map(IDLValue::Int16)
            .map_err(|_| anyhow::anyhow!("value {:?} is out of range for int16", s)),
        TypeAscription::Int32 => s
            .parse::<i32>()
            .map(IDLValue::Int32)
            .map_err(|_| anyhow::anyhow!("value {:?} is out of range for int32", s)),
        TypeAscription::Int64 => s
            .parse::<i64>()
            .map(IDLValue::Int64)
            .map_err(|_| anyhow::anyhow!("value {:?} is out of range for int64", s)),
        TypeAscription::Nat => {
            // Parse as u64 then construct Nat (handles all practical values)
            let n: u64 = s
                .parse()
                .map_err(|_| anyhow::anyhow!("value {:?} is not a valid nat", s))?;
            Ok(IDLValue::Nat(candid::Nat::from(n)))
        }
        TypeAscription::Int => {
            let n: i64 = s
                .parse()
                .map_err(|_| anyhow::anyhow!("value {:?} is not a valid int", s))?;
            Ok(IDLValue::Int(candid::Int::from(n)))
        }
        TypeAscription::Float32 => s
            .parse::<f32>()
            .map(IDLValue::Float32)
            .map_err(|_| anyhow::anyhow!("value {:?} is not a valid float32", s)),
        TypeAscription::Float64 => s
            .parse::<f64>()
            .map(IDLValue::Float64)
            .map_err(|_| anyhow::anyhow!("value {:?} is not a valid float64", s)),
    }
}

/// For `//`: if value is opt Some, unwrap it; if opt None/null, return None (filter out).
fn unwrap_opt_filter(r: IDLArgs) -> Option<IDLArgs> {
    if r.args.len() == 1 {
        match &r.args[0] {
            IDLValue::Opt(inner) => Some(IDLArgs::new(&[*inner.clone()])),
            IDLValue::None => None,
            _ => Some(r),
        }
    } else {
        Some(r)
    }
}

fn extract_field(args: &IDLArgs, name: &str, mode: OptMode) -> Result<Vec<IDLValue>> {
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
                    return apply_opt_mode(mode, &field.val, name);
                }
            }
            if mode == OptMode::Optional {
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
                return apply_opt_mode(mode, &field.val, name);
            }
            if mode == OptMode::Optional {
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

/// Apply the opt mode to a retrieved value.
fn apply_opt_mode(mode: OptMode, val: &IDLValue, field: &str) -> Result<Vec<IDLValue>> {
    match mode {
        OptMode::Normal => Ok(vec![val.clone()]),
        OptMode::Optional => match val {
            IDLValue::Opt(inner) => Ok(vec![*inner.clone()]),
            IDLValue::None => Ok(vec![]),
            _ => Ok(vec![val.clone()]),
        },
        OptMode::Assert => match val {
            IDLValue::Opt(inner) => Ok(vec![*inner.clone()]),
            IDLValue::None => bail!("field '{field}' is None; expected Some"),
            _ => Ok(vec![val.clone()]),
        },
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
