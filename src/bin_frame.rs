use anyhow::{bail, Result};

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Cursor { data, pos: 0 }
    }

    fn read_byte(&mut self) -> Result<u8> {
        if self.pos >= self.data.len() {
            bail!("unexpected end of binary Candid frame");
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn skip_n(&mut self, n: usize) -> Result<()> {
        if self.pos + n > self.data.len() {
            bail!("unexpected end of binary Candid frame");
        }
        self.pos += n;
        Ok(())
    }

    fn read_uleb(&mut self) -> Result<u64> {
        let mut result = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = self.read_byte()?;
            result |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift >= 64 {
                bail!("LEB128 overflow");
            }
        }
    }

    fn read_sleb(&mut self) -> Result<i64> {
        let mut result = 0i64;
        let mut shift = 0u32;
        loop {
            let byte = self.read_byte()?;
            result |= ((byte & 0x7f) as i64) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                if shift < 64 && (byte & 0x40) != 0 {
                    result |= -(1i64 << shift);
                }
                return Ok(result);
            }
            if shift >= 64 {
                bail!("SLEB128 overflow");
            }
        }
    }
}

// Minimal type table representation for value-skipping purposes.
// Fields store the child type refs needed to compute value sizes.
#[derive(Clone)]
enum Entry {
    Opt(i64),
    Vec(i64),
    Record(Vec<i64>),
    Variant(Vec<(u64, i64)>),
    Func,
    Service,
    Future,
}

fn read_type_table(c: &mut Cursor) -> Result<Vec<Entry>> {
    let n_types = c.read_uleb()? as usize;
    let mut table = Vec::with_capacity(n_types);
    for _ in 0..n_types {
        let code = c.read_sleb()?;
        let entry = match code {
            -18 => {
                // Opt: 1 inner type ref
                let inner = c.read_sleb()?;
                Entry::Opt(inner)
            }
            -19 => {
                // Vec: 1 inner type ref
                let inner = c.read_sleb()?;
                Entry::Vec(inner)
            }
            -20 => {
                // Record: n fields, each (leb128 hash, sleb128 type_ref)
                let n = c.read_uleb()? as usize;
                let mut fields = Vec::with_capacity(n);
                for _ in 0..n {
                    c.read_uleb()?; // field hash
                    fields.push(c.read_sleb()?); // type ref
                }
                Entry::Record(fields)
            }
            -21 => {
                // Variant: n tags, each (leb128 hash, sleb128 type_ref)
                let n = c.read_uleb()? as usize;
                let mut tags = Vec::with_capacity(n);
                for _ in 0..n {
                    let hash = c.read_uleb()?;
                    let ty = c.read_sleb()?;
                    tags.push((hash, ty));
                }
                Entry::Variant(tags)
            }
            -22 => {
                // Func: arg_count refs + ret_count refs + annotation_count (≤1) bytes
                let arg_len = c.read_uleb()? as usize;
                for _ in 0..arg_len {
                    c.read_sleb()?;
                }
                let ret_len = c.read_uleb()? as usize;
                for _ in 0..ret_len {
                    c.read_sleb()?;
                }
                let ann_len = c.read_byte()? as usize;
                for _ in 0..ann_len {
                    c.read_byte()?;
                }
                Entry::Func
            }
            -23 => {
                // Service: method_count * (name_len: leb128, name: bytes, func_type_ref: sleb128)
                let n = c.read_uleb()? as usize;
                for _ in 0..n {
                    let name_len = c.read_uleb()? as usize;
                    c.skip_n(name_len)?;
                    c.read_sleb()?;
                }
                Entry::Service
            }
            other if other < -24 => {
                // Future type: leb128 byte_count, then byte_count bytes
                let len = c.read_uleb()? as usize;
                c.skip_n(len)?;
                Entry::Future
            }
            other => bail!("unexpected type opcode {} in type table", other),
        };
        table.push(entry);
    }
    Ok(table)
}

fn skip_value(c: &mut Cursor, type_ref: i64, table: &[Entry], depth: u32) -> Result<()> {
    if depth > 1000 {
        bail!("recursion depth limit exceeded while parsing binary Candid frame");
    }
    if type_ref >= 0 {
        let idx = type_ref as usize;
        if idx >= table.len() {
            bail!("type table index {} out of range (table len {})", idx, table.len());
        }
        // Clone to avoid borrow issues with recursive calls
        let entry = table[idx].clone();
        return skip_entry_value(c, &entry, table, depth);
    }
    // Primitive type codes (negative)
    match type_ref {
        -1 => {}  // null: 0 bytes
        -2 => {
            c.skip_n(1)?; // bool: 1 byte
        }
        -3 => {
            c.read_uleb()?; // nat: unsigned LEB128
        }
        -4 => {
            c.read_sleb()?; // int: signed LEB128
        }
        -5 | -9 => {
            c.skip_n(1)?; // nat8 / int8: 1 byte
        }
        -6 | -10 => {
            c.skip_n(2)?; // nat16 / int16: 2 bytes
        }
        -7 | -11 | -13 => {
            c.skip_n(4)?; // nat32 / int32 / float32: 4 bytes
        }
        -8 | -12 | -14 => {
            c.skip_n(8)?; // nat64 / int64 / float64: 8 bytes
        }
        -15 => {
            // text: LEB128 length + bytes
            let len = c.read_uleb()? as usize;
            c.skip_n(len)?;
        }
        -16 => {} // reserved: 0 bytes
        -17 => bail!("empty type cannot have a wire value"),
        -24 => {
            // principal: 1-byte flag (must be 1) + LEB128 length + bytes
            let flag = c.read_byte()?;
            if flag != 1 {
                bail!("opaque principal reference not supported");
            }
            let len = c.read_uleb()? as usize;
            c.skip_n(len)?;
        }
        other => bail!("unknown primitive type code {} in value", other),
    }
    Ok(())
}

fn skip_entry_value(c: &mut Cursor, entry: &Entry, table: &[Entry], depth: u32) -> Result<()> {
    match entry {
        Entry::Opt(inner) => {
            let present = c.read_byte()?;
            if present == 1 {
                skip_value(c, *inner, table, depth + 1)?;
            }
        }
        Entry::Vec(inner) => {
            let count = c.read_uleb()? as usize;
            // Optimise blob (vec nat8): read all bytes at once
            if *inner == -5 {
                c.skip_n(count)?;
            } else {
                for _ in 0..count {
                    skip_value(c, *inner, table, depth + 1)?;
                }
            }
        }
        Entry::Record(fields) => {
            for &field_ref in fields {
                skip_value(c, field_ref, table, depth + 1)?;
            }
        }
        Entry::Variant(tags) => {
            let idx = c.read_uleb()? as usize;
            if idx >= tags.len() {
                bail!("variant index {} out of range (num tags {})", idx, tags.len());
            }
            skip_value(c, tags[idx].1, table, depth + 1)?;
        }
        Entry::Func => {
            // Func value: 1-byte transparent flag + principal + method-name text
            let flag = c.read_byte()?;
            if flag != 1 {
                bail!("opaque function reference not supported");
            }
            // principal
            let pflag = c.read_byte()?;
            if pflag != 1 {
                bail!("opaque principal in function reference");
            }
            let plen = c.read_uleb()? as usize;
            c.skip_n(plen)?;
            // method name
            let mlen = c.read_uleb()? as usize;
            c.skip_n(mlen)?;
        }
        Entry::Service => {
            // Service value: principal bytes
            let flag = c.read_byte()?;
            if flag != 1 {
                bail!("opaque service reference not supported");
            }
            let len = c.read_uleb()? as usize;
            c.skip_n(len)?;
        }
        Entry::Future => {
            // Future value: LEB128 count, LEB128 byte_len, then byte_len bytes
            c.read_uleb()?; // memory slot count (discard)
            let len = c.read_uleb()? as usize;
            c.skip_n(len)?;
        }
    }
    Ok(())
}

/// Returns the number of bytes occupied by the first complete DIDL frame in `bytes`.
/// Errors if the frame is truncated or malformed.
pub fn frame_size(bytes: &[u8]) -> Result<usize> {
    if bytes.len() < 4 || &bytes[..4] != b"DIDL" {
        bail!("not a DIDL binary frame (missing magic bytes)");
    }
    let mut c = Cursor::new(bytes);
    c.skip_n(4)?; // magic

    let table = read_type_table(&mut c)?;

    // Read arg-type list
    let n_args = c.read_uleb()? as usize;
    let mut arg_types = Vec::with_capacity(n_args);
    for _ in 0..n_args {
        arg_types.push(c.read_sleb()?);
    }

    // Skip each arg's value
    for type_ref in arg_types {
        skip_value(&mut c, type_ref, &table, 0)?;
    }

    Ok(c.pos)
}
