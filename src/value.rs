//! The [`Value`] data model — one MMDB data-section value.
//!
//! Every piece of data an MMDB file stores (a record payload, a map key, an array element,
//! the metadata itself) reduces to a `Value`. Construct them directly with [`Value::map`],
//! [`Value::array`], and the many [`From`] conversions, or — with the `serde` feature — let
//! [`Writer::insert`] build them from any `serde::Serialize` type.
//!
//! [`Writer::insert`]: crate::Writer::insert

use std::collections::BTreeMap;

/// One MMDB data-section value.
///
/// The variants correspond one-to-one with the data types defined by the [MMDB format].
/// There is deliberately no null/unit variant — MMDB has no null type; absence is modeled
/// by omitting a map entry.
///
/// # Equality and hashing
///
/// Floating-point variants ([`Value::Float`], [`Value::Double`]) compare and hash by their
/// raw bit pattern, so bit-identical values (including matching `NaN` payloads) are treated
/// as equal. This is what lets the writer deduplicate repeated values in the data section.
///
/// # Construction
///
/// ```
/// use mmdb_writer::Value;
///
/// let scalar = Value::from(42_u32);
/// let list = Value::array([Value::from("a"), Value::from("b")]);
/// let map = Value::map([
///     ("count", Value::from(2_u32)),
///     ("items", list),
/// ]);
/// assert!(matches!(map, Value::Map(_)));
/// ```
///
/// [MMDB format]: https://maxmind.github.io/MaxMind-DB/
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Value {
    /// UTF-8 string (type 2).
    String(String),
    /// IEEE 754 binary64 (type 3).
    Double(f64),
    /// Arbitrary byte blob (type 4).
    Bytes(Vec<u8>),
    /// Unsigned 16-bit integer (type 5).
    U16(u16),
    /// Unsigned 32-bit integer (type 6).
    U32(u32),
    /// String-keyed map (type 7). Backed by a [`BTreeMap`] so the encoded byte order — and
    /// therefore the output — is deterministic.
    Map(BTreeMap<String, Value>),
    /// Signed 32-bit integer (extended type 8).
    I32(i32),
    /// Unsigned 64-bit integer (extended type 9).
    U64(u64),
    /// Unsigned 128-bit integer (extended type 10).
    U128(u128),
    /// Ordered array (extended type 11).
    Array(Vec<Value>),
    /// Boolean (extended type 14).
    Bool(bool),
    /// IEEE 754 binary32 (extended type 15).
    Float(f32),
}

impl Value {
    /// Build a [`Value::Map`] from key/value pairs.
    ///
    /// Keys convert via [`Into<String>`] and values via [`Into<Value>`], so string literals
    /// and scalars can be passed directly.
    ///
    /// ```
    /// use mmdb_writer::Value;
    ///
    /// let v = Value::map([
    ///     ("asn", Value::from(64_512_u32)),
    ///     ("org", Value::from("Example, Inc.")),
    /// ]);
    /// ```
    pub fn map<K, V, I>(entries: I) -> Self
    where
        K: Into<String>,
        V: Into<Value>,
        I: IntoIterator<Item = (K, V)>,
    {
        Self::Map(
            entries
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
    }

    /// Build a [`Value::Array`] from a sequence of values.
    ///
    /// Elements convert via [`Into<Value>`].
    ///
    /// ```
    /// use mmdb_writer::Value;
    ///
    /// let v = Value::array([Value::from(1_u32), Value::from(2_u32)]);
    /// ```
    pub fn array<V, I>(items: I) -> Self
    where
        V: Into<Value>,
        I: IntoIterator<Item = V>,
    {
        Self::Array(items.into_iter().map(Into::into).collect())
    }

    /// Merge `new` onto `existing`, combining only the top level of two maps.
    ///
    /// If both are [`Value::Map`], the result contains every key from both; on a key present
    /// in both, `new`'s value wins (no recursion into nested maps). If either side is not a
    /// map, `new` is returned unchanged. This is the equivalent of the Go writer's
    /// `TopLevelMergeWith` inserter.
    #[must_use]
    pub fn merge_top_level(existing: &Value, new: &Value) -> Value {
        match (existing, new) {
            (Value::Map(old), Value::Map(new_map)) => {
                let mut merged = old.clone();
                for (k, v) in new_map {
                    merged.insert(k.clone(), v.clone());
                }
                Value::Map(merged)
            }
            _ => new.clone(),
        }
    }

    /// Merge `new` onto `existing`, recursing into nested maps and concatenating arrays.
    ///
    /// - Two maps merge key by key, recursing on keys present in both.
    /// - Two arrays concatenate (`existing` elements followed by `new` elements).
    /// - Any other combination yields `new`.
    ///
    /// This is the equivalent of the Go writer's `DeepMergeWith` inserter.
    #[must_use]
    pub fn merge_deep(existing: &Value, new: &Value) -> Value {
        match (existing, new) {
            (Value::Map(old), Value::Map(new_map)) => {
                let mut merged = old.clone();
                for (k, v) in new_map {
                    let combined = match merged.get(k) {
                        Some(prev) => Value::merge_deep(prev, v),
                        None => v.clone(),
                    };
                    merged.insert(k.clone(), combined);
                }
                Value::Map(merged)
            }
            (Value::Array(old), Value::Array(new_arr)) => {
                let mut merged = old.clone();
                merged.extend(new_arr.iter().cloned());
                Value::Array(merged)
            }
            _ => new.clone(),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Double(a), Self::Double(b)) => a.to_bits() == b.to_bits(),
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::U16(a), Self::U16(b)) => a == b,
            (Self::U32(a), Self::U32(b)) => a == b,
            (Self::Map(a), Self::Map(b)) => a == b,
            (Self::I32(a), Self::I32(b)) => a == b,
            (Self::U64(a), Self::U64(b)) => a == b,
            (Self::U128(a), Self::U128(b)) => a == b,
            (Self::Array(a), Self::Array(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a.to_bits() == b.to_bits(),
            _ => false,
        }
    }
}

impl Eq for Value {}

impl std::hash::Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::String(v) => v.hash(state),
            Self::Double(v) => v.to_bits().hash(state),
            Self::Bytes(v) => v.hash(state),
            Self::U16(v) => v.hash(state),
            Self::U32(v) => v.hash(state),
            Self::Map(v) => v.hash(state),
            Self::I32(v) => v.hash(state),
            Self::U64(v) => v.hash(state),
            Self::U128(v) => v.hash(state),
            Self::Array(v) => v.hash(state),
            Self::Bool(v) => v.hash(state),
            Self::Float(v) => v.to_bits().hash(state),
        }
    }
}

// --- From conversions -------------------------------------------------------------------

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::String(v.to_owned())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Self::Float(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Self::Double(v)
    }
}

// Unsigned integers map to the narrowest MMDB type that holds them; `u8`/`u16` share the
// 16-bit type, matching how the serde serializer widens them.
impl From<u8> for Value {
    fn from(v: u8) -> Self {
        Self::U16(u16::from(v))
    }
}

impl From<u16> for Value {
    fn from(v: u16) -> Self {
        Self::U16(v)
    }
}

impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Self::U32(v)
    }
}

impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Self::U64(v)
    }
}

impl From<u128> for Value {
    fn from(v: u128) -> Self {
        Self::U128(v)
    }
}

// Signed integers narrower than 64-bit share the single MMDB signed type (int32).
impl From<i8> for Value {
    fn from(v: i8) -> Self {
        Self::I32(i32::from(v))
    }
}

impl From<i16> for Value {
    fn from(v: i16) -> Self {
        Self::I32(i32::from(v))
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Self::I32(v)
    }
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Self::Bytes(v)
    }
}

impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Self {
        Self::Array(v)
    }
}

impl From<BTreeMap<String, Value>> for Value {
    fn from(v: BTreeMap<String, Value>) -> Self {
        Self::Map(v)
    }
}

impl FromIterator<Value> for Value {
    fn from_iter<I: IntoIterator<Item = Value>>(iter: I) -> Self {
        Self::Array(iter.into_iter().collect())
    }
}

impl FromIterator<(String, Value)> for Value {
    fn from_iter<I: IntoIterator<Item = (String, Value)>>(iter: I) -> Self {
        Self::Map(iter.into_iter().collect())
    }
}

#[cfg(feature = "serde")]
mod serde_impls;
