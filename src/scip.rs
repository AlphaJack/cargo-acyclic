//! Reads the occurrences of a [SCIP](https://github.com/sourcegraph/scip) index, which is Protocol Buffers.

use super::symbol::Symbol;

/// Start line, start column, end line, end column.
pub(crate) type Range = [u64; 4];

/// `S`: the symbol, as SCIP writes it or parsed.
pub(crate) struct Occurrence<S = Symbol> {
    pub(crate) range: Range,
    /// Without the scheme, the package and its version: `engine/Engine#due().`.
    pub(crate) symbol: S,
    pub(crate) definition: bool,
    /// Of a definition: the whole item, body included.
    pub(crate) enclosing: Option<Range>,
    pub(crate) in_body: bool,
    /// A C function declared in an `extern` block: no module owns it.
    pub(crate) foreign: bool,
}

/// Where a range starts, as (line, column).
pub(crate) const fn start(r: Range) -> (u64, u64) {
    let [line, column, ..] = r;
    (line, column)
}

/// Where a range ends, as (line, column).
pub(crate) const fn end(r: Range) -> (u64, u64) {
    let [.., line, column] = r;
    (line, column)
}

pub(crate) fn contains(outer: Range, inner: Range) -> bool {
    start(outer) <= start(inner) && end(inner) <= end(outer)
}

/// A file of the index.
pub(crate) struct Document<S = Symbol> {
    /// From the workspace's folder: `runtime/src/auth.rs`.
    pub(crate) path: String,
    pub(crate) occurrences: Vec<Occurrence<S>>,
}

/// The files of a SCIP index.
pub(crate) fn documents(index: &[u8]) -> Vec<Document<String>> {
    let documents = fields(index).filter_map(|field| if let (2, Value::Bytes(d)) = field { Some(d) } else { None });
    documents
        .map(|document| {
            let (mut path, mut occurrences) = (String::new(), Vec::new());
            for field in fields(document) {
                match field {
                    (1, Value::Bytes(p)) => path = String::from_utf8_lossy(p).into_owned(),
                    (2, Value::Bytes(o)) => occurrences.extend(occurrence(o)),
                    _ => {}
                }
            }
            Document { path, occurrences }
        })
        .collect()
}

/// The fields of SCIP's `Occurrence` read here (`scip.proto`).
mod occurrence_field {
    pub(super) const RANGE: u64 = 1;
    pub(super) const SYMBOL: u64 = 2;
    pub(super) const SYMBOL_ROLES: u64 = 3;
    pub(super) const ENCLOSING_RANGE: u64 = 7;
}

/// Protobuf's wire format: a key is a field number above three bits of wire type; a varint carries seven bits a byte,
/// the top one saying another follows.
mod wire {
    pub(super) const TYPE_BITS: u32 = 3;
    pub(super) const TYPE_MASK: u64 = (1 << TYPE_BITS) - 1;
    pub(super) const VARINT: u64 = 0;
    pub(super) const FIXED64: u64 = 1;
    pub(super) const LEN: u64 = 2;
    pub(super) const FIXED32: u64 = 5;
    pub(super) const VARINT_BITS: usize = 7;
    pub(super) const VARINT_VALUE: u8 = 0x7f;
    pub(super) const VARINT_MORE: u8 = 0x80;
}

fn occurrence(bytes: &[u8]) -> Option<Occurrence<String>> {
    use occurrence_field::{ENCLOSING_RANGE, RANGE, SYMBOL, SYMBOL_ROLES};
    let (mut range, mut symbol, mut definition, mut enclosing) = (None, String::new(), false, None);
    for field in fields(bytes) {
        match field {
            (RANGE, Value::Bytes(r)) => range = self::range(r),
            (SYMBOL, Value::Bytes(s)) => symbol = String::from_utf8_lossy(s).into_owned(),
            // The `Definition` bit of `symbol_roles`.
            (SYMBOL_ROLES, Value::Int(roles)) => definition = roles & 1 == 1,
            (ENCLOSING_RANGE, Value::Bytes(r)) => enclosing = self::range(r),
            _ => {}
        }
    }
    Some(Occurrence { range: range?, symbol, definition, enclosing, in_body: false, foreign: false })
}

/// From `[line, column, end column]` on one line, else `[line, column, end line, end column]`.
fn range(mut packed: &[u8]) -> Option<Range> {
    let mut values = Vec::new();
    while !packed.is_empty() {
        values.push(varint(&mut packed)?);
    }
    match values[..] {
        [line, start, end] => Some([line, start, line, end]),
        [line, start, end_line, end] => Some([line, start, end_line, end]),
        _ => None,
    }
}

enum Value<'b> {
    Int(u64),
    Bytes(&'b [u8]),
}

/// A message's field numbers and values, until the bytes end or stop making sense. Fixed-size numbers,
/// which SCIP does not use, are skipped.
fn fields(mut bytes: &[u8]) -> impl Iterator<Item = (u64, Value<'_>)> {
    std::iter::from_fn(move || {
        loop {
            let key = varint(&mut bytes)?;
            let (number, kind) = (key >> wire::TYPE_BITS, key & wire::TYPE_MASK);
            let size = match kind {
                wire::VARINT => return Some((number, Value::Int(varint(&mut bytes)?))),
                wire::FIXED64 => size_of::<u64>(),
                wire::LEN => usize::try_from(varint(&mut bytes)?).ok()?,
                wire::FIXED32 => size_of::<u32>(),
                _ => return None,
            };
            let (value, rest) = bytes.split_at_checked(size)?;
            bytes = rest;
            if kind == wire::LEN {
                return Some((number, Value::Bytes(value)));
            }
        }
    })
}

fn varint(bytes: &mut &[u8]) -> Option<u64> {
    let mut value = 0;
    for shift in (0..u64::BITS).step_by(wire::VARINT_BITS) {
        let (&byte, rest) = bytes.split_first()?;
        *bytes = rest;
        value |= u64::from(byte & wire::VARINT_VALUE) << shift;
        if byte < wire::VARINT_MORE {
            return Some(value);
        }
    }
    None
}
