use anyhow::{bail, Result};
use candid::types::Label;
use candid::types::value::{IDLField, VariantValue};
use candid::{IDLArgs, IDLValue};
use num_bigint::{BigInt, BigUint};
use std::collections::HashMap;

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

#[derive(Debug, Clone, Copy, PartialEq)]
enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BoolOp {
    And,
    Or,
}

#[derive(Debug, Clone)]
enum MatchArm {
    Tag(String),
    Default,
}

enum StrPart {
    Lit(String),
    Expr(Box<Expr>),
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
    /// Integer / bool literal
    Literal(IDLValue),
    /// `left op right` arithmetic
    BinArith(ArithOp, Box<Expr>, Box<Expr>),
    /// `left op right` comparison → bool
    BinCmp(CmpOp, Box<Expr>, Box<Expr>),
    /// `left and/or right` — boolean composition
    BinBool(BoolOp, Box<Expr>, Box<Expr>),
    /// `not expr` — boolean negation
    Not(Box<Expr>),
    /// `select(predicate)` — pass-through filter
    Select(Box<Expr>),
    /// `match { Tag1 = body1; _ = default }` — variant dispatch
    Match(Vec<(MatchArm, Box<Expr>)>),
    /// `tag(expr)` — returns the active variant tag as text
    Tag(Box<Expr>),
    /// `if cond then a [elif cond then b]* else c end`
    If(Vec<(Box<Expr>, Box<Expr>)>, Option<Box<Expr>>),
    /// `expr as $x | body` — bind expr's value to $x, evaluate body with same input
    VarBind(String, Box<Expr>, Box<Expr>),
    /// `$x` — variable reference
    VarRef(String),
    /// `"text \(expr) more"` — string interpolation
    StrInterp(Vec<StrPart>),
    /// `length` — vec element count / text char count / blob byte count
    Length,
    /// `keys` — sorted vec of record field names as text
    Keys,
    /// `values` — vec of record field values in key-sorted order
    Values,
    /// `type` — Candid type tag as text
    TypeOf,
    /// `has(name_expr)` — bool: record has named field?
    Has(Box<Expr>),
    /// `contains(val_expr)` — bool: substring for text, element for vec
    Contains(Box<Expr>),
    /// `map(filter)` — apply filter to each vec element, collect results
    Map(Box<Expr>),
    /// `to_text` — value to text string
    ToText,
    /// `to_int` — numeric / text → int
    ToInt,
    /// `to_float` — numeric / text → float64
    ToFloat,
    /// `to_principal` — text → principal
    ToPrincipal,
    /// `to_hex` — blob → hex text
    ToHex,
    /// `from_hex` — hex text → blob
    FromHex,
    /// `to_utf8` — text → blob (UTF-8 bytes)
    ToUtf8,
    /// `from_utf8` — blob → text (UTF-8 decode)
    FromUtf8,
    /// `is_some` — opt-Some → true, opt-None → false
    IsSome,
    /// `is_none` — opt-None → true, opt-Some → false
    IsNone,
    /// `sort` — sort a vec by natural ordering
    Sort,
    /// `sort_by(filter)` — sort a vec by the filter's value over each element
    SortBy(Box<Expr>),
    /// `group_by(filter)` — group elements by filter value; returns vec-of-vecs
    GroupBy(Box<Expr>),
    /// `unique` — deduplicate a vec preserving first-occurrence order
    Unique,
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
// Also handles: ascribe as $x | pipe
fn parse_pipe(s: &str) -> Result<(Expr, &str)> {
    let (left, rest) = parse_ascribe(s)?;
    let rest = rest.trim_start();

    // Check for 'as $x | body' before plain pipe
    if let Some(after_as) = rest.strip_prefix("as") {
        if after_as
            .chars()
            .next()
            .map_or(true, |c| !c.is_alphanumeric() && c != '_')
        {
            let after_as = after_as.trim_start();
            if let Some(after_dollar) = after_as.strip_prefix('$') {
                let ident_end = after_dollar
                    .find(|c: char| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(after_dollar.len());
                if ident_end > 0 {
                    let name = after_dollar[..ident_end].to_string();
                    let after_name = after_dollar[ident_end..].trim_start();
                    if let Some(after_pipe) = after_name.strip_prefix('|') {
                        if !after_pipe.starts_with('/') {
                            let (body, rest2) = parse_pipe(after_pipe.trim_start())?;
                            return Ok((
                                Expr::VarBind(name, Box::new(left), Box::new(body)),
                                rest2,
                            ));
                        }
                    }
                }
            }
        }
    }

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

// ascribe: alt_expr (: TypeName)?
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

// alt: or_expr // or_expr — unwrap opt or use fallback
fn parse_alt(s: &str) -> Result<(Expr, &str)> {
    let (left, rest) = parse_or(s)?;
    let rest = rest.trim_start();
    if let Some(after) = rest.strip_prefix("//") {
        let (right, rest2) = parse_or(after.trim_start())?;
        Ok((Expr::Alt(Box::new(left), Box::new(right)), rest2))
    } else {
        Ok((left, rest))
    }
}

// or: and (or and)*
fn parse_or(s: &str) -> Result<(Expr, &str)> {
    let (mut left, mut rest) = parse_and(s)?;
    loop {
        let r = rest.trim_start();
        if let Some(after) = r.strip_prefix("or") {
            if after
                .chars()
                .next()
                .map_or(true, |c| !c.is_alphanumeric() && c != '_')
            {
                let (right, rest2) = parse_and(after.trim_start())?;
                left = Expr::BinBool(BoolOp::Or, Box::new(left), Box::new(right));
                rest = rest2;
                continue;
            }
        }
        break;
    }
    Ok((left, rest))
}

// and: not_expr (and not_expr)*
fn parse_and(s: &str) -> Result<(Expr, &str)> {
    let (mut left, mut rest) = parse_not(s)?;
    loop {
        let r = rest.trim_start();
        if let Some(after) = r.strip_prefix("and") {
            if after
                .chars()
                .next()
                .map_or(true, |c| !c.is_alphanumeric() && c != '_')
            {
                let (right, rest2) = parse_not(after.trim_start())?;
                left = Expr::BinBool(BoolOp::And, Box::new(left), Box::new(right));
                rest = rest2;
                continue;
            }
        }
        break;
    }
    Ok((left, rest))
}

// not: "not" not_expr | cmp
fn parse_not(s: &str) -> Result<(Expr, &str)> {
    let s = s.trim_start();
    if let Some(after) = s.strip_prefix("not") {
        if after
            .chars()
            .next()
            .map_or(true, |c| !c.is_alphanumeric() && c != '_')
        {
            let (inner, rest) = parse_not(after.trim_start())?;
            return Ok((Expr::Not(Box::new(inner)), rest));
        }
    }
    parse_cmp(s)
}

// cmp: add op add (single comparison, no chaining)
fn parse_cmp(s: &str) -> Result<(Expr, &str)> {
    let (left, rest) = parse_add(s)?;
    let rest_t = rest.trim_start();

    // Check two-char operators before one-char to avoid ambiguity
    let (op, after_op) = if let Some(r) = rest_t.strip_prefix("==") {
        (CmpOp::Eq, r)
    } else if let Some(r) = rest_t.strip_prefix("!=") {
        (CmpOp::Ne, r)
    } else if let Some(r) = rest_t.strip_prefix("<=") {
        (CmpOp::Le, r)
    } else if let Some(r) = rest_t.strip_prefix(">=") {
        (CmpOp::Ge, r)
    } else if let Some(r) = rest_t.strip_prefix('<') {
        (CmpOp::Lt, r)
    } else if let Some(r) = rest_t.strip_prefix('>') {
        (CmpOp::Gt, r)
    } else {
        return Ok((left, rest));
    };

    let (right, rest2) = parse_add(after_op.trim_start())?;
    Ok((Expr::BinCmp(op, Box::new(left), Box::new(right)), rest2))
}

// add: mul ((+ | -) mul)*
fn parse_add(s: &str) -> Result<(Expr, &str)> {
    let (mut left, mut rest) = parse_mul(s)?;
    loop {
        let r = rest.trim_start();
        if let Some(after) = r.strip_prefix('+') {
            let (right, rest2) = parse_mul(after.trim_start())?;
            left = Expr::BinArith(ArithOp::Add, Box::new(left), Box::new(right));
            rest = rest2;
        } else if r.starts_with('-') && !r.starts_with("->") {
            let after = &r[1..];
            let (right, rest2) = parse_mul(after.trim_start())?;
            left = Expr::BinArith(ArithOp::Sub, Box::new(left), Box::new(right));
            rest = rest2;
        } else {
            break;
        }
    }
    Ok((left, rest))
}

// mul: atom ((* | / | %) atom)*
fn parse_mul(s: &str) -> Result<(Expr, &str)> {
    let (mut left, mut rest) = parse_atom(s)?;
    loop {
        let r = rest.trim_start();
        if let Some(after) = r.strip_prefix('*') {
            let (right, rest2) = parse_atom(after.trim_start())?;
            left = Expr::BinArith(ArithOp::Mul, Box::new(left), Box::new(right));
            rest = rest2;
        } else if r.starts_with('/') && !r.starts_with("//") {
            let after = &r[1..];
            let (right, rest2) = parse_atom(after.trim_start())?;
            left = Expr::BinArith(ArithOp::Div, Box::new(left), Box::new(right));
            rest = rest2;
        } else if let Some(after) = r.strip_prefix('%') {
            let (right, rest2) = parse_atom(after.trim_start())?;
            left = Expr::BinArith(ArithOp::Rem, Box::new(left), Box::new(right));
            rest = rest2;
        } else {
            break;
        }
    }
    Ok((left, rest))
}

fn keyword_boundary(s: &str) -> bool {
    s.chars()
        .next()
        .map_or(true, |c| !c.is_alphanumeric() && c != '_')
}

// atom: literals | if | $x | select | some | none | principal | blob | variant | record | vec | dotchain
fn parse_atom(s: &str) -> Result<(Expr, &str)> {
    let s = s.trim_start();

    // Parenthesised sub-expression
    if let Some(after) = s.strip_prefix('(') {
        let (inner, rest) = parse_pipe(after.trim_start())?;
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix(')')
            .ok_or_else(|| anyhow::anyhow!("expected ')' to close parenthesised expression"))?;
        return Ok((inner, rest));
    }

    // `if cond then a [elif cond then b]* else c end`
    if let Some(after) = s.strip_prefix("if") {
        if keyword_boundary(after) {
            return parse_if(after.trim_start());
        }
    }

    // `$x` — variable reference
    if let Some(after) = s.strip_prefix('$') {
        let ident_end = after
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(after.len());
        if ident_end > 0 {
            let name = after[..ident_end].to_string();
            return Ok((Expr::VarRef(name), &after[ident_end..]));
        }
    }

    // `true` / `false` boolean literals
    if let Some(after) = s.strip_prefix("true") {
        if keyword_boundary(after) {
            return Ok((Expr::Literal(IDLValue::Bool(true)), after));
        }
    }
    if let Some(after) = s.strip_prefix("false") {
        if keyword_boundary(after) {
            return Ok((Expr::Literal(IDLValue::Bool(false)), after));
        }
    }

    // Non-negative integer literal: digits only
    if s.chars().next().map_or(false, |c| c.is_ascii_digit()) {
        let num_end = s
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(s.len());
        let num_str = &s[..num_end];
        let rest = &s[num_end..];
        return Ok((Expr::Literal(IDLValue::Number(num_str.to_string())), rest));
    }

    // String literal with optional interpolation `"..."` or `"text \(expr) more"`
    if s.starts_with('"') {
        let (parts, rest) = parse_interp_string(s)?;
        if parts.iter().all(|p| matches!(p, StrPart::Lit(_))) {
            let text: String = parts
                .into_iter()
                .map(|p| match p {
                    StrPart::Lit(s) => s,
                    StrPart::Expr(_) => unreachable!(),
                })
                .collect();
            return Ok((Expr::Literal(IDLValue::Text(text)), rest));
        }
        return Ok((Expr::StrInterp(parts), rest));
    }

    // `select(pipe)`
    if let Some(after) = s.strip_prefix("select(") {
        let (pred, rest) = parse_pipe(after.trim_start())?;
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix(')')
            .ok_or_else(|| anyhow::anyhow!("expected ')' after select(...)"))?;
        return Ok((Expr::Select(Box::new(pred)), rest));
    }

    // `none` keyword (not followed by alphanumeric or '_')
    if let Some(after) = s.strip_prefix("none") {
        if keyword_boundary(after) {
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

    // `match { Tag1 = body1; Tag2 = body2; _ = default }` — variant dispatch
    if let Some(after) = s.strip_prefix("match") {
        if keyword_boundary(after) {
            let after = after.trim_start();
            if after.starts_with('{') {
                let (arms, rest) = parse_match_arms(&after[1..])?;
                return Ok((Expr::Match(arms), rest));
            }
        }
    }

    // `tag(expr)` — returns the active variant tag as text
    if let Some(after) = s.strip_prefix("tag(") {
        let (inner, rest) = parse_pipe(after.trim_start())?;
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix(')')
            .ok_or_else(|| anyhow::anyhow!("expected ')' after tag(...)"))?;
        return Ok((Expr::Tag(Box::new(inner)), rest));
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

    // --- Generic builtins (longer names before shorter to avoid prefix shadowing) ---

    // `from_hex` / `from_utf8` — must check `from_` before `from` alone
    if let Some(after) = s.strip_prefix("from_hex") {
        if keyword_boundary(after) {
            return Ok((Expr::FromHex, after));
        }
    }
    if let Some(after) = s.strip_prefix("from_utf8") {
        if keyword_boundary(after) {
            return Ok((Expr::FromUtf8, after));
        }
    }

    // `to_principal` before `to_` prefix alternatives
    if let Some(after) = s.strip_prefix("to_principal") {
        if keyword_boundary(after) {
            return Ok((Expr::ToPrincipal, after));
        }
    }
    if let Some(after) = s.strip_prefix("to_float") {
        if keyword_boundary(after) {
            return Ok((Expr::ToFloat, after));
        }
    }
    if let Some(after) = s.strip_prefix("to_text") {
        if keyword_boundary(after) {
            return Ok((Expr::ToText, after));
        }
    }
    if let Some(after) = s.strip_prefix("to_utf8") {
        if keyword_boundary(after) {
            return Ok((Expr::ToUtf8, after));
        }
    }
    if let Some(after) = s.strip_prefix("to_hex") {
        if keyword_boundary(after) {
            return Ok((Expr::ToHex, after));
        }
    }
    if let Some(after) = s.strip_prefix("to_int") {
        if keyword_boundary(after) {
            return Ok((Expr::ToInt, after));
        }
    }

    // `is_some` / `is_none`
    if let Some(after) = s.strip_prefix("is_some") {
        if keyword_boundary(after) {
            return Ok((Expr::IsSome, after));
        }
    }
    if let Some(after) = s.strip_prefix("is_none") {
        if keyword_boundary(after) {
            return Ok((Expr::IsNone, after));
        }
    }

    // `length` / `keys` / `values` / `type`
    if let Some(after) = s.strip_prefix("length") {
        if keyword_boundary(after) {
            return Ok((Expr::Length, after));
        }
    }
    if let Some(after) = s.strip_prefix("keys") {
        if keyword_boundary(after) {
            return Ok((Expr::Keys, after));
        }
    }
    if let Some(after) = s.strip_prefix("values") {
        if keyword_boundary(after) {
            return Ok((Expr::Values, after));
        }
    }
    if let Some(after) = s.strip_prefix("type") {
        if keyword_boundary(after) {
            return Ok((Expr::TypeOf, after));
        }
    }

    // `sort_by(filter)` before `sort` to avoid prefix collision
    if let Some(after) = s.strip_prefix("sort_by(") {
        let (inner, rest) = parse_pipe(after.trim_start())?;
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix(')')
            .ok_or_else(|| anyhow::anyhow!("expected ')' after sort_by(...)"))?;
        return Ok((Expr::SortBy(Box::new(inner)), rest));
    }
    if let Some(after) = s.strip_prefix("sort") {
        if keyword_boundary(after) {
            return Ok((Expr::Sort, after));
        }
    }
    if let Some(after) = s.strip_prefix("group_by(") {
        let (inner, rest) = parse_pipe(after.trim_start())?;
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix(')')
            .ok_or_else(|| anyhow::anyhow!("expected ')' after group_by(...)"))?;
        return Ok((Expr::GroupBy(Box::new(inner)), rest));
    }
    if let Some(after) = s.strip_prefix("unique") {
        if keyword_boundary(after) {
            return Ok((Expr::Unique, after));
        }
    }

    // `has(expr)` / `contains(expr)` / `map(expr)`
    if let Some(after) = s.strip_prefix("has(") {
        let (inner, rest) = parse_pipe(after.trim_start())?;
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix(')')
            .ok_or_else(|| anyhow::anyhow!("expected ')' after has(...)"))?;
        return Ok((Expr::Has(Box::new(inner)), rest));
    }
    if let Some(after) = s.strip_prefix("contains(") {
        let (inner, rest) = parse_pipe(after.trim_start())?;
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix(')')
            .ok_or_else(|| anyhow::anyhow!("expected ')' after contains(...)"))?;
        return Ok((Expr::Contains(Box::new(inner)), rest));
    }
    if let Some(after) = s.strip_prefix("map(") {
        let (inner, rest) = parse_pipe(after.trim_start())?;
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix(')')
            .ok_or_else(|| anyhow::anyhow!("expected ')' after map(...)"))?;
        return Ok((Expr::Map(Box::new(inner)), rest));
    }

    // fallthrough to dotchain
    parse_dotchain(s)
}

// Parse `if cond then body [elif cond then body]* else body end` — caller consumed 'if'
fn parse_if(s: &str) -> Result<(Expr, &str)> {
    let mut branches: Vec<(Box<Expr>, Box<Expr>)> = Vec::new();
    let mut rest = s;

    // Parse first condition
    let (cond, r) = parse_pipe(rest)?;
    rest = r.trim_start();
    rest = expect_keyword(rest, "then", "expected 'then' after if condition")?;
    let (then_body, r) = parse_pipe(rest.trim_start())?;
    rest = r;
    branches.push((Box::new(cond), Box::new(then_body)));

    // Parse elif chains
    loop {
        let r = rest.trim_start();
        if let Some(after_elif) = r.strip_prefix("elif") {
            if keyword_boundary(after_elif) {
                let (cond, r2) = parse_pipe(after_elif.trim_start())?;
                let r2 = r2.trim_start();
                let r2 = expect_keyword(r2, "then", "expected 'then' after elif condition")?;
                let (then_body, r3) = parse_pipe(r2.trim_start())?;
                branches.push((Box::new(cond), Box::new(then_body)));
                rest = r3;
                continue;
            }
        }
        break;
    }

    // Parse else branch (required)
    rest = rest.trim_start();
    let else_branch = if let Some(after_else) = rest.strip_prefix("else") {
        if keyword_boundary(after_else) {
            let (else_body, r) = parse_pipe(after_else.trim_start())?;
            rest = r;
            Some(Box::new(else_body))
        } else {
            None
        }
    } else {
        None
    };

    rest = rest.trim_start();
    rest = expect_keyword(rest, "end", "expected 'end' to close if expression")?;

    Ok((Expr::If(branches, else_branch), rest))
}

/// Consume a keyword at the start of `s`, returning the remaining string.
fn expect_keyword<'a>(s: &'a str, kw: &str, msg: &str) -> Result<&'a str> {
    if let Some(after) = s.strip_prefix(kw) {
        if keyword_boundary(after) {
            return Ok(after);
        }
    }
    bail!("{msg}")
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

// Parse `{ Tag1 = body1; Tag2 = body2; _ = default }` — caller consumed the leading `{`
fn parse_match_arms(s: &str) -> Result<(Vec<(MatchArm, Box<Expr>)>, &str)> {
    let mut arms: Vec<(MatchArm, Box<Expr>)> = Vec::new();
    let mut rest = s.trim_start();
    if let Some(after) = rest.strip_prefix('}') {
        return Ok((arms, after));
    }
    loop {
        rest = rest.trim_start();
        // Parse arm name: `_` for default, or an identifier for a tag
        let arm = if rest.starts_with('_') {
            let after = &rest[1..];
            if after
                .chars()
                .next()
                .map_or(true, |c| !c.is_alphanumeric() && c != '_')
            {
                rest = after;
                MatchArm::Default
            } else {
                let ident_end = rest
                    .find(|c: char| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(rest.len());
                let name = rest[..ident_end].to_string();
                rest = &rest[ident_end..];
                MatchArm::Tag(name)
            }
        } else {
            let ident_end = rest
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            if ident_end == 0 {
                bail!("expected tag name or '_' in match arm, got {:?}", rest);
            }
            let name = rest[..ident_end].to_string();
            rest = &rest[ident_end..];
            MatchArm::Tag(name)
        };
        rest = rest.trim_start();
        rest = rest
            .strip_prefix('=')
            .ok_or_else(|| anyhow::anyhow!("expected '=' after arm name in match"))?;
        let (body, after_body) = parse_pipe(rest.trim_start())?;
        arms.push((arm, Box::new(body)));
        rest = after_body.trim_start();
        if let Some(after) = rest.strip_prefix('}') {
            return Ok((arms, after));
        } else if let Some(after) = rest.strip_prefix(';') {
            rest = after;
        } else {
            bail!("expected ';' or '}}' in match expression, got {:?}", rest);
        }
    }
}

// Parse a double-quoted string with optional \(expr) interpolation segments.
// Returns a Vec<StrPart> where each part is a literal or an expression.
fn parse_interp_string(s: &str) -> Result<(Vec<StrPart>, &str)> {
    let mut remaining = s
        .strip_prefix('"')
        .ok_or_else(|| anyhow::anyhow!("expected '\"'"))?;
    let mut parts: Vec<StrPart> = Vec::new();
    let mut lit = String::new();

    loop {
        if remaining.is_empty() {
            bail!("unterminated string literal");
        }
        match remaining.as_bytes()[0] {
            b'"' => {
                if !lit.is_empty() {
                    parts.push(StrPart::Lit(std::mem::take(&mut lit)));
                }
                return Ok((parts, &remaining[1..]));
            }
            b'\\' => {
                if remaining.len() < 2 {
                    bail!("unterminated escape in string literal");
                }
                let esc = remaining.as_bytes()[1];
                if esc == b'(' {
                    // String interpolation: \(expr)
                    if !lit.is_empty() {
                        parts.push(StrPart::Lit(std::mem::take(&mut lit)));
                    }
                    let (expr, rest) = parse_pipe(&remaining[2..])?;
                    let rest = rest.trim_start();
                    let rest = rest.strip_prefix(')').ok_or_else(|| {
                        anyhow::anyhow!("expected ')' to close \\(...) in string interpolation")
                    })?;
                    parts.push(StrPart::Expr(Box::new(expr)));
                    remaining = rest;
                } else {
                    match esc {
                        b'n' => {
                            lit.push('\n');
                            remaining = &remaining[2..];
                        }
                        b't' => {
                            lit.push('\t');
                            remaining = &remaining[2..];
                        }
                        b'r' => {
                            lit.push('\r');
                            remaining = &remaining[2..];
                        }
                        b'"' => {
                            lit.push('"');
                            remaining = &remaining[2..];
                        }
                        b'\\' => {
                            lit.push('\\');
                            remaining = &remaining[2..];
                        }
                        hi @ (b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F') => {
                            if remaining.len() < 3 {
                                bail!("incomplete hex escape in string literal");
                            }
                            let lo = remaining.as_bytes()[2];
                            let byte_val = hex_nibble(hi)? << 4 | hex_nibble(lo)?;
                            lit.push(byte_val as char);
                            remaining = &remaining[3..];
                        }
                        other => bail!("unknown string escape '\\{}'", other as char),
                    }
                }
            }
            _ => {
                let ch = remaining.chars().next().unwrap();
                lit.push(ch);
                remaining = &remaining[ch.len_utf8()..];
            }
        }
    }
}

// Parse a double-quoted string returning raw bytes (no interpolation support).
// Used for `blob "..."` and `principal "..."` where raw bytes are needed.
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
// Numeric intermediate representation (bigint-during-evaluation)
// ---------------------------------------------------------------------------

enum Num {
    Unsigned(BigUint),
    Signed(BigInt),
    Float(f64),
}

fn to_num(val: &IDLValue) -> Result<Num> {
    match val {
        IDLValue::Nat(n) => Ok(Num::Unsigned(n.0.clone())),
        IDLValue::Int(n) => Ok(Num::Signed(n.0.clone())),
        IDLValue::Nat8(n) => Ok(Num::Unsigned(BigUint::from(*n as u64))),
        IDLValue::Nat16(n) => Ok(Num::Unsigned(BigUint::from(*n as u64))),
        IDLValue::Nat32(n) => Ok(Num::Unsigned(BigUint::from(*n as u64))),
        IDLValue::Nat64(n) => Ok(Num::Unsigned(BigUint::from(*n))),
        IDLValue::Int8(n) => Ok(Num::Signed(BigInt::from(*n as i64))),
        IDLValue::Int16(n) => Ok(Num::Signed(BigInt::from(*n as i64))),
        IDLValue::Int32(n) => Ok(Num::Signed(BigInt::from(*n as i64))),
        IDLValue::Int64(n) => Ok(Num::Signed(BigInt::from(*n))),
        IDLValue::Float32(f) => Ok(Num::Float(*f as f64)),
        IDLValue::Float64(f) => Ok(Num::Float(*f)),
        IDLValue::Number(s) => {
            if s.starts_with('-') {
                let n: BigInt = s
                    .parse()
                    .map_err(|_| anyhow::anyhow!("cannot parse number {:?}", s))?;
                Ok(Num::Signed(n))
            } else {
                let n: BigUint = s
                    .parse()
                    .map_err(|_| anyhow::anyhow!("cannot parse number {:?}", s))?;
                Ok(Num::Unsigned(n))
            }
        }
        other => bail!(
            "arithmetic requires a numeric value, got {}",
            type_name(other)
        ),
    }
}

fn from_num(n: Num) -> IDLValue {
    match n {
        Num::Unsigned(u) => IDLValue::Nat(candid::Nat(u)),
        Num::Signed(i) => IDLValue::Int(candid::Int(i)),
        Num::Float(f) => IDLValue::Float64(f),
    }
}

fn num_to_bigint(n: Num) -> BigInt {
    match n {
        Num::Signed(i) => i,
        Num::Unsigned(u) => BigInt::from(u),
        Num::Float(_) => panic!("float should not reach num_to_bigint"),
    }
}

fn eval_arith(op: ArithOp, a: Num, b: Num) -> Result<Num> {
    match (a, b) {
        // Float ↔ int mixing is an error
        (Num::Float(_), Num::Unsigned(_))
        | (Num::Float(_), Num::Signed(_))
        | (Num::Unsigned(_), Num::Float(_))
        | (Num::Signed(_), Num::Float(_)) => bail!(
            "cannot mix float and integer in arithmetic; use to_float or to_int for conversion"
        ),
        (Num::Float(af), Num::Float(bf)) => {
            let result = match op {
                ArithOp::Add => af + bf,
                ArithOp::Sub => af - bf,
                ArithOp::Mul => af * bf,
                ArithOp::Div => af / bf,
                ArithOp::Rem => af % bf,
            };
            Ok(Num::Float(result))
        }
        (Num::Unsigned(a), Num::Unsigned(b)) => match op {
            ArithOp::Add => Ok(Num::Unsigned(a + b)),
            ArithOp::Sub => {
                if a >= b {
                    Ok(Num::Unsigned(a - b))
                } else {
                    // nat - nat with negative result: widen to signed
                    let ai = BigInt::from(a);
                    let bi = BigInt::from(b);
                    Ok(Num::Signed(ai - bi))
                }
            }
            ArithOp::Mul => Ok(Num::Unsigned(a * b)),
            ArithOp::Div => {
                if b == BigUint::from(0u64) {
                    bail!("division by zero")
                }
                Ok(Num::Unsigned(a / b))
            }
            ArithOp::Rem => {
                if b == BigUint::from(0u64) {
                    bail!("modulo by zero")
                }
                Ok(Num::Unsigned(a % b))
            }
        },
        (a, b) => {
            // One or both signed: convert to BigInt
            let ai = num_to_bigint(a);
            let bi = num_to_bigint(b);
            match op {
                ArithOp::Add => Ok(Num::Signed(ai + bi)),
                ArithOp::Sub => Ok(Num::Signed(ai - bi)),
                ArithOp::Mul => Ok(Num::Signed(ai * bi)),
                ArithOp::Div => {
                    if bi == BigInt::from(0i64) {
                        bail!("division by zero")
                    }
                    Ok(Num::Signed(ai / bi))
                }
                ArithOp::Rem => {
                    if bi == BigInt::from(0i64) {
                        bail!("modulo by zero")
                    }
                    Ok(Num::Signed(ai % bi))
                }
            }
        }
    }
}

fn eval_cmp(op: CmpOp, a: Num, b: Num) -> Result<bool> {
    match (a, b) {
        (Num::Float(_), Num::Unsigned(_))
        | (Num::Float(_), Num::Signed(_))
        | (Num::Unsigned(_), Num::Float(_))
        | (Num::Signed(_), Num::Float(_)) => bail!(
            "cannot compare float and integer; use to_float or to_int for conversion"
        ),
        (Num::Float(af), Num::Float(bf)) => Ok(match op {
            CmpOp::Eq => af == bf,
            CmpOp::Ne => af != bf,
            CmpOp::Lt => af < bf,
            CmpOp::Gt => af > bf,
            CmpOp::Le => af <= bf,
            CmpOp::Ge => af >= bf,
        }),
        (a, b) => {
            // Convert both to BigInt for mixed signed/unsigned comparison
            let ai = num_to_bigint(a);
            let bi = num_to_bigint(b);
            Ok(match op {
                CmpOp::Eq => ai == bi,
                CmpOp::Ne => ai != bi,
                CmpOp::Lt => ai < bi,
                CmpOp::Gt => ai > bi,
                CmpOp::Le => ai <= bi,
                CmpOp::Ge => ai >= bi,
            })
        }
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
    eval_expr(&query, args, &HashMap::new())
}

fn eval_expr(
    expr: &Expr,
    args: IDLArgs,
    env: &HashMap<String, IDLValue>,
) -> Result<Vec<IDLArgs>> {
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
            for item in eval_expr(left, args, env)? {
                results.extend(eval_expr(right, item, env)?);
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
            let results = eval_expr(left, args.clone(), env)?;
            let unwrapped: Vec<IDLArgs> = results
                .into_iter()
                .filter_map(unwrap_opt_filter)
                .collect();
            if unwrapped.is_empty() {
                eval_expr(right, args, env)
            } else {
                Ok(unwrapped)
            }
        }
        Expr::SomeOf(inner) => {
            let results = eval_expr(inner, args, env)?;
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
                let mut results = eval_expr(field_expr, args.clone(), env)?;
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
                let mut results = eval_expr(elem_expr, args.clone(), env)?;
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
                let mut results = eval_expr(expr, args, env)?;
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
            let mut results = eval_expr(inner, args, env)?;
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
            let results = eval_expr(inner, args, env)?;
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

        Expr::Literal(v) => Ok(vec![IDLArgs::new(&[v.clone()])]),

        Expr::BinArith(op, left, right) => {
            let l_results = eval_expr(left, args.clone(), env)?;
            if l_results.len() != 1 || l_results[0].args.len() != 1 {
                bail!("arithmetic requires a single value on the left side");
            }
            let r_results = eval_expr(right, args, env)?;
            if r_results.len() != 1 || r_results[0].args.len() != 1 {
                bail!("arithmetic requires a single value on the right side");
            }
            let lv = to_num(&l_results[0].args[0])?;
            let rv = to_num(&r_results[0].args[0])?;
            let result = eval_arith(*op, lv, rv)?;
            Ok(vec![IDLArgs::new(&[from_num(result)])])
        }

        Expr::BinCmp(op, left, right) => {
            let l_results = eval_expr(left, args.clone(), env)?;
            if l_results.len() != 1 || l_results[0].args.len() != 1 {
                bail!("comparison requires a single value on the left side");
            }
            let r_results = eval_expr(right, args, env)?;
            if r_results.len() != 1 || r_results[0].args.len() != 1 {
                bail!("comparison requires a single value on the right side");
            }
            let lval = &l_results[0].args[0];
            let rval = &r_results[0].args[0];
            if let (IDLValue::Text(ls), IDLValue::Text(rs)) = (lval, rval) {
                let result = match op {
                    CmpOp::Eq => ls == rs,
                    CmpOp::Ne => ls != rs,
                    CmpOp::Lt => ls < rs,
                    CmpOp::Gt => ls > rs,
                    CmpOp::Le => ls <= rs,
                    CmpOp::Ge => ls >= rs,
                };
                return Ok(vec![IDLArgs::new(&[IDLValue::Bool(result)])]);
            }
            if let (IDLValue::Principal(lp), IDLValue::Principal(rp)) = (lval, rval) {
                let ls = lp.to_text();
                let rs = rp.to_text();
                let result = match op {
                    CmpOp::Eq => ls == rs,
                    CmpOp::Ne => ls != rs,
                    CmpOp::Lt => ls < rs,
                    CmpOp::Gt => ls > rs,
                    CmpOp::Le => ls <= rs,
                    CmpOp::Ge => ls >= rs,
                };
                return Ok(vec![IDLArgs::new(&[IDLValue::Bool(result)])]);
            }
            let lv = to_num(lval)?;
            let rv = to_num(rval)?;
            let result = eval_cmp(*op, lv, rv)?;
            Ok(vec![IDLArgs::new(&[IDLValue::Bool(result)])])
        }

        Expr::BinBool(op, left, right) => {
            let lb = eval_single_bool(left, args.clone(), env)?;
            let rb = eval_single_bool(right, args, env)?;
            let result = match op {
                BoolOp::And => lb && rb,
                BoolOp::Or => lb || rb,
            };
            Ok(vec![IDLArgs::new(&[IDLValue::Bool(result)])])
        }

        Expr::Not(inner) => {
            let b = eval_single_bool(inner, args, env)?;
            Ok(vec![IDLArgs::new(&[IDLValue::Bool(!b)])])
        }

        Expr::Select(pred) => {
            let pred_result = eval_expr(pred, args.clone(), env)?;
            if pred_result.is_empty() {
                return Ok(vec![]);
            }
            if pred_result.len() != 1 || pred_result[0].args.len() != 1 {
                bail!("select predicate must produce a single boolean value");
            }
            match &pred_result[0].args[0] {
                IDLValue::Bool(true) => Ok(vec![args]),
                IDLValue::Bool(false) => Ok(vec![]),
                other => bail!(
                    "select predicate must return bool, got {}",
                    type_name(other)
                ),
            }
        }

        Expr::Match(arms) => {
            if args.args.len() != 1 {
                bail!(
                    "match requires a single value, got {} values",
                    args.args.len()
                );
            }
            let variant = match &args.args[0] {
                IDLValue::Variant(v) => v,
                other => bail!("match requires a variant value, got {}", type_name(other)),
            };
            let active_tag = label_display(&variant.0.id);
            let payload = variant.0.val.clone();
            for (arm, body) in arms {
                let matches = match arm {
                    MatchArm::Tag(name) => {
                        let h = candid::idl_hash(name);
                        label_matches(&variant.0.id, name, h)
                    }
                    MatchArm::Default => true,
                };
                if matches {
                    return eval_expr(body, IDLArgs::new(&[payload]), env);
                }
            }
            bail!(
                "no match arm for variant tag '{active_tag}'; add a '_ = ...' default arm"
            )
        }

        Expr::Tag(inner) => {
            let results = eval_expr(inner, args, env)?;
            if results.len() != 1 || results[0].args.len() != 1 {
                bail!("tag() requires a single value");
            }
            match &results[0].args[0] {
                IDLValue::Variant(v) => {
                    let tag = label_display(&v.0.id);
                    Ok(vec![IDLArgs::new(&[IDLValue::Text(tag)])])
                }
                other => bail!(
                    "tag() requires a variant value, got {}",
                    type_name(other)
                ),
            }
        }

        Expr::If(branches, else_branch) => {
            for (cond, then_body) in branches {
                let cond_result = eval_expr(cond, args.clone(), env)?;
                if cond_result.len() != 1 || cond_result[0].args.len() != 1 {
                    bail!("if condition must produce a single boolean value");
                }
                match &cond_result[0].args[0] {
                    IDLValue::Bool(true) => return eval_expr(then_body, args, env),
                    IDLValue::Bool(false) => continue,
                    other => bail!(
                        "if condition must be a bool, got {}",
                        type_name(other)
                    ),
                }
            }
            if let Some(else_body) = else_branch {
                eval_expr(else_body, args, env)
            } else {
                Ok(vec![])
            }
        }

        Expr::VarBind(name, bound_expr, body) => {
            let bound_results = eval_expr(bound_expr, args.clone(), env)?;
            let mut results = Vec::new();
            for r in bound_results {
                if r.args.len() != 1 {
                    bail!(
                        "'as ${name}' binding requires exactly one value, got {}",
                        r.args.len()
                    );
                }
                let mut new_env = env.clone();
                new_env.insert(name.clone(), r.args[0].clone());
                results.extend(eval_expr(body, args.clone(), &new_env)?);
            }
            Ok(results)
        }

        Expr::VarRef(name) => {
            let val = env
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("undefined variable ${name}"))?;
            Ok(vec![IDLArgs::new(&[val.clone()])])
        }

        Expr::StrInterp(parts) => {
            let mut result = String::new();
            for part in parts {
                match part {
                    StrPart::Lit(s) => result.push_str(s),
                    StrPart::Expr(e) => {
                        let vals = eval_expr(e, args.clone(), env)?;
                        if vals.len() != 1 || vals[0].args.len() != 1 {
                            bail!(
                                "string interpolation \\(...) must produce exactly one value, got {}",
                                vals.len()
                            );
                        }
                        result.push_str(&idl_to_text(&vals[0].args[0]));
                    }
                }
            }
            Ok(vec![IDLArgs::new(&[IDLValue::Text(result)])])
        }

        Expr::Length => {
            let val = require_single_val(&args, "length")?;
            let n: u64 = match &val {
                IDLValue::Vec(items) => items.len() as u64,
                IDLValue::Text(s) => s.chars().count() as u64,
                IDLValue::Blob(bytes) => bytes.len() as u64,
                other => bail!("length requires vec, text, or blob, got {}", type_name(other)),
            };
            Ok(vec![IDLArgs::new(&[IDLValue::Nat(candid::Nat::from(n))])])
        }

        Expr::Keys => {
            let val = require_single_val(&args, "keys")?;
            match val {
                IDLValue::Record(mut fields) => {
                    fields.sort_by(|a, b| label_display(&a.id).cmp(&label_display(&b.id)));
                    let keys: Vec<IDLValue> = fields
                        .iter()
                        .map(|f| IDLValue::Text(label_display(&f.id)))
                        .collect();
                    Ok(vec![IDLArgs::new(&[IDLValue::Vec(keys)])])
                }
                other => bail!("keys requires a record, got {}", type_name(&other)),
            }
        }

        Expr::Values => {
            let val = require_single_val(&args, "values")?;
            match val {
                IDLValue::Record(mut fields) => {
                    fields.sort_by(|a, b| label_display(&a.id).cmp(&label_display(&b.id)));
                    let vals: Vec<IDLValue> = fields.into_iter().map(|f| f.val).collect();
                    Ok(vec![IDLArgs::new(&[IDLValue::Vec(vals)])])
                }
                other => bail!("values requires a record, got {}", type_name(&other)),
            }
        }

        Expr::TypeOf => {
            let val = require_single_val(&args, "type")?;
            Ok(vec![IDLArgs::new(&[IDLValue::Text(
                type_tag(&val).to_string(),
            )])])
        }

        Expr::Has(name_expr) => {
            let name_result = eval_expr(name_expr, args.clone(), env)?;
            if name_result.len() != 1 || name_result[0].args.len() != 1 {
                bail!("has() requires a single name value");
            }
            let name = match &name_result[0].args[0] {
                IDLValue::Text(s) => s.clone(),
                other => bail!("has() requires a text name, got {}", type_name(other)),
            };
            let val = require_single_val(&args, "has")?;
            match val {
                IDLValue::Record(fields) => {
                    let hash = candid::idl_hash(&name);
                    let found = fields.iter().any(|f| label_matches(&f.id, &name, hash));
                    Ok(vec![IDLArgs::new(&[IDLValue::Bool(found)])])
                }
                other => bail!("has() requires a record, got {}", type_name(&other)),
            }
        }

        Expr::Contains(val_expr) => {
            let needle_result = eval_expr(val_expr, args.clone(), env)?;
            if needle_result.len() != 1 || needle_result[0].args.len() != 1 {
                bail!("contains() requires a single value argument");
            }
            let needle = &needle_result[0].args[0];
            let val = require_single_val(&args, "contains")?;
            match &val {
                IDLValue::Text(s) => match needle {
                    IDLValue::Text(sub) => {
                        Ok(vec![IDLArgs::new(&[IDLValue::Bool(s.contains(sub.as_str()))])])
                    }
                    other => bail!(
                        "contains() on text requires a text argument, got {}",
                        type_name(other)
                    ),
                },
                IDLValue::Vec(items) => {
                    let found = items.iter().any(|item| item == needle);
                    Ok(vec![IDLArgs::new(&[IDLValue::Bool(found)])])
                }
                other => bail!(
                    "contains() requires text or vec, got {}",
                    type_name(other)
                ),
            }
        }

        Expr::Map(filter) => {
            let val = require_single_val(&args, "map")?;
            match val {
                IDLValue::Vec(items) => {
                    let mut results: Vec<IDLValue> = Vec::new();
                    for item in items {
                        let item_args = IDLArgs::new(&[item]);
                        for r in eval_expr(filter, item_args, env)? {
                            if r.args.len() != 1 {
                                bail!("map filter must produce single values per element");
                            }
                            results.push(r.args.into_iter().next().unwrap());
                        }
                    }
                    Ok(vec![IDLArgs::new(&[IDLValue::Vec(results)])])
                }
                other => bail!("map requires a vec, got {}", type_name(&other)),
            }
        }

        Expr::ToText => {
            let val = require_single_val(&args, "to_text")?;
            Ok(vec![IDLArgs::new(&[IDLValue::Text(idl_to_text(&val))])])
        }

        Expr::ToInt => {
            let val = require_single_val(&args, "to_int")?;
            let n: BigInt = match &val {
                IDLValue::Int(n) => n.0.clone(),
                IDLValue::Nat(n) => BigInt::from(n.0.clone()),
                IDLValue::Number(s) => s
                    .parse::<BigInt>()
                    .map_err(|_| anyhow::anyhow!("to_int: cannot parse {:?} as integer", s))?,
                IDLValue::Nat8(n) => BigInt::from(*n as i64),
                IDLValue::Nat16(n) => BigInt::from(*n as i64),
                IDLValue::Nat32(n) => BigInt::from(*n as i64),
                IDLValue::Nat64(n) => BigInt::from(*n as i64),
                IDLValue::Int8(n) => BigInt::from(*n as i64),
                IDLValue::Int16(n) => BigInt::from(*n as i64),
                IDLValue::Int32(n) => BigInt::from(*n as i64),
                IDLValue::Int64(n) => BigInt::from(*n as i64),
                IDLValue::Float32(f) => BigInt::from(*f as i64),
                IDLValue::Float64(f) => BigInt::from(*f as i64),
                IDLValue::Text(s) => s
                    .parse::<BigInt>()
                    .map_err(|_| anyhow::anyhow!("to_int: cannot parse {:?} as integer", s))?,
                other => bail!("to_int: cannot convert {} to int", type_name(other)),
            };
            Ok(vec![IDLArgs::new(&[IDLValue::Int(candid::Int(n))])])
        }

        Expr::ToFloat => {
            let val = require_single_val(&args, "to_float")?;
            let f: f64 = match &val {
                IDLValue::Float64(f) => *f,
                IDLValue::Float32(f) => *f as f64,
                IDLValue::Nat(n) => n
                    .to_string()
                    .parse::<f64>()
                    .map_err(|_| anyhow::anyhow!("to_float: cannot convert nat to f64"))?,
                IDLValue::Int(n) => n
                    .to_string()
                    .parse::<f64>()
                    .map_err(|_| anyhow::anyhow!("to_float: cannot convert int to f64"))?,
                IDLValue::Number(s) => s
                    .parse::<f64>()
                    .map_err(|_| anyhow::anyhow!("to_float: cannot parse {:?} as float", s))?,
                IDLValue::Nat8(n) => *n as f64,
                IDLValue::Nat16(n) => *n as f64,
                IDLValue::Nat32(n) => *n as f64,
                IDLValue::Nat64(n) => *n as f64,
                IDLValue::Int8(n) => *n as f64,
                IDLValue::Int16(n) => *n as f64,
                IDLValue::Int32(n) => *n as f64,
                IDLValue::Int64(n) => *n as f64,
                IDLValue::Text(s) => s
                    .parse::<f64>()
                    .map_err(|_| anyhow::anyhow!("to_float: cannot parse {:?} as float", s))?,
                other => bail!("to_float: cannot convert {} to float", type_name(other)),
            };
            Ok(vec![IDLArgs::new(&[IDLValue::Float64(f)])])
        }

        Expr::ToPrincipal => {
            let val = require_single_val(&args, "to_principal")?;
            match val {
                IDLValue::Text(s) => {
                    let p = candid::Principal::from_text(&s)
                        .map_err(|e| anyhow::anyhow!("to_principal: invalid principal {:?}: {e}", s))?;
                    Ok(vec![IDLArgs::new(&[IDLValue::Principal(p)])])
                }
                other => bail!("to_principal requires text, got {}", type_name(&other)),
            }
        }

        Expr::ToHex => {
            let val = require_single_val(&args, "to_hex")?;
            match val {
                IDLValue::Blob(bytes) => {
                    Ok(vec![IDLArgs::new(&[IDLValue::Text(hex::encode(&bytes))])])
                }
                other => bail!("to_hex requires blob, got {}", type_name(&other)),
            }
        }

        Expr::FromHex => {
            let val = require_single_val(&args, "from_hex")?;
            match val {
                IDLValue::Text(s) => {
                    let bytes = hex::decode(&s)
                        .map_err(|e| anyhow::anyhow!("from_hex: invalid hex string: {e}"))?;
                    Ok(vec![IDLArgs::new(&[IDLValue::Blob(bytes)])])
                }
                other => bail!("from_hex requires text, got {}", type_name(&other)),
            }
        }

        Expr::ToUtf8 => {
            let val = require_single_val(&args, "to_utf8")?;
            match val {
                IDLValue::Text(s) => {
                    Ok(vec![IDLArgs::new(&[IDLValue::Blob(s.into_bytes())])])
                }
                other => bail!("to_utf8 requires text, got {}", type_name(&other)),
            }
        }

        Expr::FromUtf8 => {
            let val = require_single_val(&args, "from_utf8")?;
            match val {
                IDLValue::Blob(bytes) => {
                    let s = String::from_utf8(bytes)
                        .map_err(|_| anyhow::anyhow!("from_utf8: blob is not valid UTF-8"))?;
                    Ok(vec![IDLArgs::new(&[IDLValue::Text(s)])])
                }
                other => bail!("from_utf8 requires blob, got {}", type_name(&other)),
            }
        }

        Expr::IsSome => {
            let val = require_single_val(&args, "is_some")?;
            match val {
                IDLValue::Opt(_) => Ok(vec![IDLArgs::new(&[IDLValue::Bool(true)])]),
                IDLValue::None => Ok(vec![IDLArgs::new(&[IDLValue::Bool(false)])]),
                other => bail!("is_some requires an opt value, got {}", type_name(&other)),
            }
        }

        Expr::IsNone => {
            let val = require_single_val(&args, "is_none")?;
            match val {
                IDLValue::Opt(_) => Ok(vec![IDLArgs::new(&[IDLValue::Bool(false)])]),
                IDLValue::None => Ok(vec![IDLArgs::new(&[IDLValue::Bool(true)])]),
                other => bail!("is_none requires an opt value, got {}", type_name(&other)),
            }
        }

        Expr::Sort => {
            let val = require_single_val(&args, "sort")?;
            match val {
                IDLValue::Vec(mut items) => {
                    let mut sort_err: Option<anyhow::Error> = None;
                    items.sort_by(|a, b| {
                        cmp_idl_values(a, b).unwrap_or_else(|e| {
                            sort_err = Some(e);
                            std::cmp::Ordering::Equal
                        })
                    });
                    if let Some(e) = sort_err {
                        return Err(e);
                    }
                    Ok(vec![IDLArgs::new(&[IDLValue::Vec(items)])])
                }
                other => bail!("sort requires a vec, got {}", type_name(&other)),
            }
        }

        Expr::SortBy(filter) => {
            let val = require_single_val(&args, "sort_by")?;
            match val {
                IDLValue::Vec(items) => {
                    let mut keyed: Vec<(IDLValue, IDLValue)> = items
                        .into_iter()
                        .map(|item| {
                            let item_args = IDLArgs::new(&[item.clone()]);
                            let key_results = eval_expr(filter, item_args, env)?;
                            if key_results.len() != 1 || key_results[0].args.len() != 1 {
                                bail!("sort_by filter must produce exactly one value per element");
                            }
                            Ok((key_results.into_iter().next().unwrap().args.into_iter().next().unwrap(), item))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let mut sort_err: Option<anyhow::Error> = None;
                    keyed.sort_by(|(ka, _), (kb, _)| {
                        cmp_idl_values(ka, kb).unwrap_or_else(|e| {
                            sort_err = Some(e);
                            std::cmp::Ordering::Equal
                        })
                    });
                    if let Some(e) = sort_err {
                        return Err(e);
                    }
                    let sorted: Vec<IDLValue> = keyed.into_iter().map(|(_, v)| v).collect();
                    Ok(vec![IDLArgs::new(&[IDLValue::Vec(sorted)])])
                }
                other => bail!("sort_by requires a vec, got {}", type_name(&other)),
            }
        }

        Expr::GroupBy(filter) => {
            let val = require_single_val(&args, "group_by")?;
            match val {
                IDLValue::Vec(items) => {
                    let mut groups: Vec<(IDLValue, Vec<IDLValue>)> = Vec::new();
                    for item in items {
                        let item_args = IDLArgs::new(&[item.clone()]);
                        let key_results = eval_expr(filter, item_args, env)?;
                        if key_results.len() != 1 || key_results[0].args.len() != 1 {
                            bail!("group_by filter must produce exactly one value per element");
                        }
                        let key = key_results.into_iter().next().unwrap().args.into_iter().next().unwrap();
                        if let Some((_, group)) = groups.iter_mut().find(|(k, _)| k == &key) {
                            group.push(item);
                        } else {
                            groups.push((key, vec![item]));
                        }
                    }
                    let result: Vec<IDLValue> = groups
                        .into_iter()
                        .map(|(_, group)| IDLValue::Vec(group))
                        .collect();
                    Ok(vec![IDLArgs::new(&[IDLValue::Vec(result)])])
                }
                other => bail!("group_by requires a vec, got {}", type_name(&other)),
            }
        }

        Expr::Unique => {
            let val = require_single_val(&args, "unique")?;
            match val {
                IDLValue::Vec(items) => {
                    let mut seen: Vec<IDLValue> = Vec::new();
                    let result: Vec<IDLValue> = items
                        .into_iter()
                        .filter(|item| {
                            if seen.iter().any(|s| s == item) {
                                false
                            } else {
                                seen.push(item.clone());
                                true
                            }
                        })
                        .collect();
                    Ok(vec![IDLArgs::new(&[IDLValue::Vec(result)])])
                }
                other => bail!("unique requires a vec, got {}", type_name(&other)),
            }
        }
    }
}

fn cmp_idl_values(a: &IDLValue, b: &IDLValue) -> Result<std::cmp::Ordering> {
    match (a, b) {
        (IDLValue::Null | IDLValue::None, IDLValue::Null | IDLValue::None) => {
            Ok(std::cmp::Ordering::Equal)
        }
        (IDLValue::Bool(a), IDLValue::Bool(b)) => Ok(a.cmp(b)),
        (IDLValue::Text(a), IDLValue::Text(b)) => Ok(a.cmp(b)),
        (IDLValue::Principal(a), IDLValue::Principal(b)) => {
            Ok(a.to_text().cmp(&b.to_text()))
        }
        _ => {
            let na = to_num(a).map_err(|_| {
                anyhow::anyhow!(
                    "cannot compare {} and {}: incompatible types for sort",
                    type_name(a),
                    type_name(b)
                )
            })?;
            let nb = to_num(b).map_err(|_| {
                anyhow::anyhow!(
                    "cannot compare {} and {}: incompatible types for sort",
                    type_name(a),
                    type_name(b)
                )
            })?;
            match (na, nb) {
                (Num::Float(fa), Num::Float(fb)) => {
                    Ok(fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal))
                }
                (Num::Float(_), _) | (_, Num::Float(_)) => bail!(
                    "cannot mix float and integer in sort; use to_float or to_int"
                ),
                (a, b) => {
                    let ai = num_to_bigint(a);
                    let bi = num_to_bigint(b);
                    Ok(ai.cmp(&bi))
                }
            }
        }
    }
}

fn eval_single_bool(
    expr: &Expr,
    args: IDLArgs,
    env: &HashMap<String, IDLValue>,
) -> Result<bool> {
    let results = eval_expr(expr, args, env)?;
    if results.len() != 1 || results[0].args.len() != 1 {
        bail!("boolean operation requires a single boolean value");
    }
    match &results[0].args[0] {
        IDLValue::Bool(b) => Ok(*b),
        other => bail!("expected bool, got {}", type_name(other)),
    }
}

/// Convert an IDLValue to a plain text string (for string interpolation).
/// Text values are returned as-is; numeric values give their numeric string;
/// other values use their Candid text representation.
fn idl_to_text(val: &IDLValue) -> String {
    match val {
        IDLValue::Text(s) => s.clone(),
        IDLValue::Bool(b) => b.to_string(),
        IDLValue::Nat(n) => n.to_string(),
        IDLValue::Int(n) => n.to_string(),
        IDLValue::Number(s) => s.clone(),
        IDLValue::Nat8(n) => n.to_string(),
        IDLValue::Nat16(n) => n.to_string(),
        IDLValue::Nat32(n) => n.to_string(),
        IDLValue::Nat64(n) => n.to_string(),
        IDLValue::Int8(n) => n.to_string(),
        IDLValue::Int16(n) => n.to_string(),
        IDLValue::Int32(n) => n.to_string(),
        IDLValue::Int64(n) => n.to_string(),
        IDLValue::Float32(f) => f.to_string(),
        IDLValue::Float64(f) => f.to_string(),
        IDLValue::Null | IDLValue::None => "null".to_string(),
        other => format!("{other}"),
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
        IDLValue::Record(fields) => {
            if i > u32::MAX as usize {
                bail!("hash index {i} is out of range for a 32-bit field hash");
            }
            let hash = i as u32;
            for field in fields {
                let matches = match &field.id {
                    Label::Id(n) | Label::Unnamed(n) => *n == hash,
                    Label::Named(n) => candid::idl_hash(n) == hash,
                };
                if matches {
                    return Ok(field.val.clone());
                }
            }
            bail!("no field with hash {i} in record");
        }
        other => bail!(
            "index access '.[{i}]' requires a vec or record, got {}",
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

fn require_single_val(args: &IDLArgs, ctx: &str) -> Result<IDLValue> {
    if args.args.len() != 1 {
        bail!(
            "'{ctx}' requires a single value, got {} values",
            args.args.len()
        );
    }
    Ok(args.args[0].clone())
}

fn type_tag(val: &IDLValue) -> &'static str {
    match val {
        IDLValue::Bool(_) => "bool",
        IDLValue::Null | IDLValue::None => "null",
        IDLValue::Text(_) => "text",
        IDLValue::Number(_) => "number",
        IDLValue::Float32(_) => "float32",
        IDLValue::Float64(_) => "float64",
        IDLValue::Opt(_) => "opt",
        IDLValue::Vec(_) => "vec",
        IDLValue::Record(_) => "record",
        IDLValue::Variant(_) => "variant",
        IDLValue::Blob(_) => "blob",
        IDLValue::Principal(_) => "principal",
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
        IDLValue::Service(_) => "service",
        IDLValue::Func(_, _) => "func",
        IDLValue::Reserved => "reserved",
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
