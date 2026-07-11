// Narrowing casts (`x as u8`, `x as u16`) are intentional throughout this module — we are
// packing numeric values into fixed-width bytes per the MMDB wire format, and the size
// classes range-check the values first. Suppressing the pedantic truncation lints keeps the
// bit-manipulation readable.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

//! Encode [`Value`]s into MMDB's data-section byte format, with pointer deduplication.
//!
//! The encoder is a stateful accumulator: append values via [`DataSection::push`], and each
//! repeated `Value` collapses to a small pointer instead of being re-emitted. The dedup
//! table is where most of the format's on-disk compactness comes from — every record in a
//! cloud-provider feed shares the same `{provider: "aws"}` map, and it is stored once.
//!
//! Spec reference: <https://maxmind.github.io/MaxMind-DB/> §3.5.

use std::collections::HashMap;

use crate::value::Value;

/// Offset into the data section, counted from the first byte *after* the 16-null separator.
pub(crate) type DataOffset = u32;

/// Type tag in the control byte's high 3 bits. Extended types all use `Extended` and are
/// disambiguated via the first byte of the payload (see [`ExtendedType`]).
#[derive(Debug, Clone, Copy)]
enum TypeTag {
    Extended = 0,
    Pointer = 1,
    Utf8 = 2,
    Double = 3,
    Bytes = 4,
    Uint16 = 5,
    Uint32 = 6,
    Map = 7,
}

/// Extended-type indicator byte. The spec adds 7 to this value to get the "real" type number.
#[derive(Debug, Clone, Copy)]
enum ExtendedType {
    Int32 = 1,
    Uint64 = 2,
    Uint128 = 3,
    Array = 4,
    Bool = 7,
    Float = 8,
}

/// Metadata cached alongside each deduplicated `Value`: the offset of its first emission and
/// the byte length of that emission. The length decides pointer-vs-inline when re-emitting —
/// a type-1 pointer costs 2–5 bytes, so inline wins for very small values.
#[derive(Debug, Clone, Copy)]
struct DedupEntry {
    offset: DataOffset,
    len: u32,
}

/// Stateful data-section encoder with pointer deduplication.
pub(crate) struct DataSection {
    /// Raw encoded bytes. Grows monotonically.
    bytes: Vec<u8>,
    /// Dedup table: `Value` → (offset of first appearance, byte length of that emission).
    dedup: HashMap<Value, DedupEntry>,
    /// When set, values are always emitted inline and never replaced by pointers.
    no_pointers: bool,
}

impl DataSection {
    pub(crate) fn new() -> Self {
        Self {
            bytes: Vec::new(),
            dedup: HashMap::new(),
            no_pointers: false,
        }
    }

    /// A data section that never emits pointers (fully inline output).
    pub(crate) fn without_pointers() -> Self {
        Self {
            no_pointers: true,
            ..Self::new()
        }
    }

    /// Emit `value` at the end of the section and return the offset at which it lives. If
    /// `value` was previously emitted, returns the original offset without re-emitting —
    /// tree records always prefer the existing direct offset over a pointer to it.
    pub(crate) fn push(&mut self, value: &Value) -> DataOffset {
        if let Some(&entry) = self.dedup.get(value) {
            return entry.offset;
        }
        let offset = u32_len(&self.bytes);
        self.encode(value);
        let len = u32_len(&self.bytes).saturating_sub(offset);
        self.dedup.insert(value.clone(), DedupEntry { offset, len });
        offset
    }

    /// Consume the encoder and return the encoded bytes.
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Current length of the encoded section, in bytes.
    pub(crate) fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Emit `value` inline at the current position inside a parent container (map value or
    /// array element). If a prior copy is cached and a pointer to it is smaller than a fresh
    /// inline encoding, emit a type-1 pointer; otherwise re-emit inline. The size check keeps
    /// tiny repeated values (1-byte bools, empty strings) from being replaced by 2-byte
    /// pointers. Matches Go's `WriteOrWritePointer`.
    fn emit_inline(&mut self, value: &Value) {
        if !self.no_pointers {
            if let Some(&entry) = self.dedup.get(value) {
                let pointer_size = u32::from(pointer_bytes(entry.offset));
                if entry.len > pointer_size {
                    self.write_pointer(entry.offset);
                    return;
                }
                // Cached value is small enough that inline wins; fall through to re-emit.
            }
        }
        let offset = u32_len(&self.bytes);
        self.encode(value);
        let len = u32_len(&self.bytes).saturating_sub(offset);
        // Keep the first emission's offset (lower offset → smaller pointers later).
        self.dedup
            .entry(value.clone())
            .or_insert(DedupEntry { offset, len });
    }

    fn encode(&mut self, value: &Value) {
        match value {
            Value::String(s) => self.write_blob(TypeTag::Utf8, s.as_bytes()),
            Value::Double(d) => {
                self.write_control_byte(TypeTag::Double, 8);
                self.bytes.extend_from_slice(&d.to_be_bytes());
            }
            Value::Bytes(b) => self.write_blob(TypeTag::Bytes, b),
            Value::U16(n) => self.write_uint(TypeTag::Uint16, u128::from(*n), 2),
            Value::U32(n) => self.write_uint(TypeTag::Uint32, u128::from(*n), 4),
            Value::Map(m) => {
                self.write_control_byte(TypeTag::Map, m.len());
                for (k, v) in m {
                    // Map keys are strings; emit inline and go through dedup so repeated key
                    // strings across records coalesce.
                    self.emit_inline(&Value::String(k.clone()));
                    self.emit_inline(v);
                }
            }
            Value::I32(n) => {
                self.write_extended(ExtendedType::Int32, |buf| encode_i32_trimmed(buf, *n));
            }
            Value::U64(n) => self.write_extended(ExtendedType::Uint64, |buf| {
                encode_uint_trimmed(buf, u128::from(*n), 8)
            }),
            Value::U128(n) => {
                self.write_extended(ExtendedType::Uint128, |buf| {
                    encode_uint_trimmed(buf, *n, 16)
                });
            }
            Value::Array(items) => {
                self.write_extended_header(ExtendedType::Array, items.len());
                for item in items {
                    self.emit_inline(item);
                }
            }
            Value::Bool(b) => {
                // Booleans encode their value in the size field: 0 = false, 1 = true.
                self.write_extended_header(ExtendedType::Bool, usize::from(*b));
            }
            Value::Float(f) => {
                self.write_extended_header(ExtendedType::Float, 4);
                self.bytes.extend_from_slice(&f.to_be_bytes());
            }
        }
    }

    fn write_blob(&mut self, tag: TypeTag, data: &[u8]) {
        self.write_control_byte(tag, data.len());
        self.bytes.extend_from_slice(data);
    }

    fn write_uint(&mut self, tag: TypeTag, value: u128, max_bytes: usize) {
        let mut buf = Vec::with_capacity(max_bytes);
        let size = encode_uint_trimmed(&mut buf, value, max_bytes);
        self.write_control_byte(tag, size);
        self.bytes.extend_from_slice(&buf);
    }

    fn write_extended<F>(&mut self, ext: ExtendedType, write_payload: F)
    where
        F: FnOnce(&mut Vec<u8>) -> usize,
    {
        let mut payload = Vec::new();
        let size = write_payload(&mut payload);
        self.write_extended_header(ext, size);
        self.bytes.extend_from_slice(&payload);
    }

    fn write_extended_header(&mut self, ext: ExtendedType, size: usize) {
        self.write_control_byte(TypeTag::Extended, size);
        self.bytes.push(ext as u8);
    }

    /// Emit the control byte(s) encoding (type, size). Handles the four size-field layouts
    /// from spec §3.5: inline (0..29), 1-byte extension (29..285), 2-byte extension
    /// (285..65_821), 3-byte extension (65_821..).
    fn write_control_byte(&mut self, tag: TypeTag, size: usize) {
        let tag_bits = (tag as u8) << 5;
        if size < 29 {
            self.bytes.push(tag_bits | (size as u8));
        // 0x1d / 0x1e are the spec extension markers for 1-byte and 2-byte size fields.
        } else if size < 29 + 256 {
            self.bytes.push(tag_bits | 0x1d);
            let delta = (size - 29) as u8;
            self.bytes.push(delta);
        } else if size < 285 + 65_536 {
            self.bytes.push(tag_bits | 0x1e);
            let delta = (size - 285) as u16;
            self.bytes.extend_from_slice(&delta.to_be_bytes());
        } else {
            self.bytes.push(tag_bits | 31);
            let delta_u32 = u32_from_usize(size.saturating_sub(65_821));
            let bytes = delta_u32.to_be_bytes();
            // 24-bit extension (only the low 3 bytes of the u32).
            self.bytes.extend_from_slice(&bytes[1..4]);
        }
    }

    /// Encode a type-1 pointer back to an earlier data-section offset. The pointer size class
    /// (spec §3.5.1) is chosen to minimise bytes written.
    fn write_pointer(&mut self, offset: DataOffset) {
        let tag_bits = (TypeTag::Pointer as u8) << 5;
        let addr = u64::from(offset);
        if addr < 2_048 {
            let high = ((addr >> 8) & 0b111) as u8;
            let low = (addr & 0xFF) as u8;
            self.bytes.push(tag_bits | high);
            self.bytes.push(low);
        } else if addr < 526_336 {
            let biased = addr - 2_048;
            let high = ((biased >> 16) & 0b111) as u8;
            let mid = ((biased >> 8) & 0xFF) as u8;
            let low = (biased & 0xFF) as u8;
            self.bytes.push(tag_bits | (1 << 3) | high);
            self.bytes.push(mid);
            self.bytes.push(low);
        } else if addr < 134_744_064 {
            let biased = addr - 526_336;
            let high = ((biased >> 24) & 0b111) as u8;
            let b2 = ((biased >> 16) & 0xFF) as u8;
            let b1 = ((biased >> 8) & 0xFF) as u8;
            let b0 = (biased & 0xFF) as u8;
            self.bytes.push(tag_bits | (2 << 3) | high);
            self.bytes.push(b2);
            self.bytes.push(b1);
            self.bytes.push(b0);
        } else {
            self.bytes.push(tag_bits | (3 << 3));
            let raw = (addr as u32).to_be_bytes();
            self.bytes.extend_from_slice(&raw);
        }
    }
}

/// Emit `value` trimmed of leading zero bytes, big-endian, into `buf`. Returns the number of
/// bytes written. `max_bytes` bounds the representation width (e.g. 8 for u64).
fn encode_uint_trimmed(buf: &mut Vec<u8>, value: u128, max_bytes: usize) -> usize {
    if value == 0 {
        return 0;
    }
    let full = value.to_be_bytes();
    let start = full.len().saturating_sub(max_bytes);
    let slice = &full[start..];
    let leading_zeros = slice.iter().take_while(|b| **b == 0).count();
    buf.extend_from_slice(&slice[leading_zeros..]);
    slice.len() - leading_zeros
}

/// Trim a signed 32-bit value to its minimal two's-complement big-endian representation.
/// Returns the number of bytes written. Zero writes nothing (size = 0 in the control byte).
fn encode_i32_trimmed(buf: &mut Vec<u8>, value: i32) -> usize {
    if value == 0 {
        return 0;
    }
    let full = value.to_be_bytes();
    let pad = if value < 0 { 0xFF_u8 } else { 0 };
    let mut start = 0;
    while start + 1 < full.len() && full[start] == pad {
        // Keep at least one byte whose sign bit agrees with the value's sign.
        let next_msb_set = full[start + 1] & 0x80 != 0;
        let would_flip_sign = (value < 0) ^ next_msb_set;
        if would_flip_sign {
            break;
        }
        start += 1;
    }
    buf.extend_from_slice(&full[start..]);
    full.len() - start
}

/// Byte length of the type-1 pointer encoding for `offset`. Mirrors the size classes in
/// [`DataSection::write_pointer`]: 2/3/4/5 bytes for the four ranges.
fn pointer_bytes(offset: DataOffset) -> u8 {
    let addr = u64::from(offset);
    if addr < 2_048 {
        2
    } else if addr < 526_336 {
        3
    } else if addr < 134_744_064 {
        4
    } else {
        5
    }
}

fn u32_len(v: &[u8]) -> u32 {
    u32_from_usize(v.len())
}

fn u32_from_usize(v: usize) -> u32 {
    debug_assert!(u32::try_from(v).is_ok(), "mmdb offsets must fit in u32");
    u32::try_from(v).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(value: &Value) -> Vec<u8> {
        let mut section = DataSection::new();
        section.push(value);
        section.into_bytes()
    }

    #[test]
    fn small_string_control_byte() {
        // "Foo" — type 2 (string), size 3: control byte 0b010_00011 = 0x43.
        assert_eq!(
            encode(&Value::String("Foo".into())),
            vec![0x43, b'F', b'o', b'o']
        );
    }

    #[test]
    fn empty_string_is_just_control_byte() {
        assert_eq!(encode(&Value::String(String::new())), vec![0x40]);
    }

    #[test]
    fn uint16_trims_leading_zeros() {
        // 0 → size 0, no payload: 0b101_00000 = 0xA0.
        assert_eq!(encode(&Value::U16(0)), vec![0xA0]);
        // 1 → size 1, one payload byte: 0xA1, 0x01.
        assert_eq!(encode(&Value::U16(1)), vec![0xA1, 0x01]);
        // 0x01FF → size 2: 0xA2, 0x01, 0xFF.
        assert_eq!(encode(&Value::U16(0x01FF)), vec![0xA2, 0x01, 0xFF]);
    }

    #[test]
    fn uint32_encoding() {
        // 500 = 0x01F4 → type 6, size 2.
        assert_eq!(encode(&Value::U32(500)), vec![0xC2, 0x01, 0xF4]);
    }

    #[test]
    fn bool_encoded_in_size_field() {
        // Extended type, size field carries the value; control byte 0b000_ssss then ext=7.
        assert_eq!(encode(&Value::Bool(false)), vec![0x00, 0x07]);
        assert_eq!(encode(&Value::Bool(true)), vec![0x01, 0x07]);
    }

    #[test]
    fn double_is_eight_bytes_be() {
        let mut want = vec![0x68]; // type 3, size 8.
        want.extend_from_slice(&1.5_f64.to_be_bytes());
        assert_eq!(encode(&Value::Double(1.5)), want);
    }

    #[test]
    fn float_is_four_bytes_be_extended() {
        let mut want = vec![0x04, 0x08]; // extended, size 4; ext type 8 (float).
        want.extend_from_slice(&1.5_f32.to_be_bytes());
        assert_eq!(encode(&Value::Float(1.5)), want);
    }

    #[test]
    fn int32_minimal_twos_complement() {
        assert_eq!(encode(&Value::I32(0)), vec![0x00, 0x01]); // size 0
        assert_eq!(encode(&Value::I32(-1)), vec![0x01, 0x01, 0xFF]); // one byte 0xFF
        assert_eq!(encode(&Value::I32(-256)), vec![0x02, 0x01, 0xFF, 0x00]);
        // 128 needs a leading zero byte so the sign bit stays positive.
        assert_eq!(encode(&Value::I32(128)), vec![0x02, 0x01, 0x00, 0x80]);
    }

    #[test]
    fn control_byte_size_class_boundaries() {
        // Size 28 fits inline; size 29 spills to the 1-byte extension (delta = 0).
        let s28 = "a".repeat(28);
        assert_eq!(encode(&Value::String(s28))[0], 0x40 | 28);
        let s29 = "a".repeat(29);
        let out = encode(&Value::String(s29));
        assert_eq!(out[0], 0x40 | 0x1d);
        assert_eq!(out[1], 0); // 29 - 29
        // Size 285 is the first 2-byte extension (delta = 0).
        let s285 = "a".repeat(285);
        let out = encode(&Value::String(s285));
        assert_eq!(out[0], 0x40 | 0x1e);
        assert_eq!(&out[1..3], &[0x00, 0x00]);
    }

    #[test]
    fn map_encodes_len_in_control_byte() {
        let v = Value::map([("a", Value::U16(1))]);
        let out = encode(&v);
        // type 7 (map), size 1: 0b111_00001 = 0xE1, then key "a" (0x41 'a'), then value.
        assert_eq!(out[0], 0xE1);
        assert_eq!(&out[1..3], &[0x41, b'a']);
        assert_eq!(&out[3..], &[0xA1, 0x01]);
    }

    #[test]
    fn repeated_large_value_dedups_to_pointer() {
        // Two array elements sharing a big string: the second must be a pointer, not a copy.
        let big = Value::String("x".repeat(50));
        let v = Value::array([big.clone(), big]);
        let out = encode(&v);
        // Array header (ext, size 2, ext type 4) then first string inline (52 bytes) then a
        // 2-byte pointer back to offset 2.
        assert_eq!(&out[0..2], &[0x02, 0x04]);
        // First element control byte: string, size 50 → 1-byte extension.
        assert_eq!(out[2], 0x40 | 0x1d);
        assert_eq!(out[3], (50 - 29) as u8);
        // Total = 2 (array hdr) + 2 (str hdr) + 50 (payload) + 2 (pointer) = 56.
        assert_eq!(out.len(), 56);
        // Pointer type is 1: high 3 bits == 0b001.
        assert_eq!(out[54] >> 5, 1);
    }

    #[test]
    fn pointer_size_classes() {
        assert_eq!(pointer_bytes(0), 2);
        assert_eq!(pointer_bytes(2_047), 2);
        assert_eq!(pointer_bytes(2_048), 3);
        assert_eq!(pointer_bytes(526_335), 3);
        assert_eq!(pointer_bytes(526_336), 4);
        assert_eq!(pointer_bytes(134_744_063), 4);
        assert_eq!(pointer_bytes(134_744_064), 5);
    }
}
