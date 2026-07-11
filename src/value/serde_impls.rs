//! `Serialize`/`Deserialize` for [`Value`], gated on the `serde` feature.
//!
//! A `Value` serializes *as* the data it holds (a [`Value::String`] serializes as a string,
//! a [`Value::Map`] as a map) rather than as a tagged enum, and deserializes from arbitrary
//! self-describing data into the closest-fitting variant. The `Deserialize` side is what
//! lets [`Writer::load`] rebuild values from an existing database.
//!
//! [`Writer::load`]: crate::Writer::load

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};

use super::Value;

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::String(s) => serializer.serialize_str(s),
            Value::Double(d) => serializer.serialize_f64(*d),
            Value::Bytes(b) => serializer.serialize_bytes(b),
            Value::U16(n) => serializer.serialize_u16(*n),
            Value::U32(n) => serializer.serialize_u32(*n),
            Value::I32(n) => serializer.serialize_i32(*n),
            Value::U64(n) => serializer.serialize_u64(*n),
            Value::U128(n) => serializer.serialize_u128(*n),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Float(f) => serializer.serialize_f32(*f),
            Value::Map(m) => {
                let mut map = serializer.serialize_map(Some(m.len()))?;
                for (k, v) in m {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
            Value::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any MMDB-representable value")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i8<E: de::Error>(self, v: i8) -> Result<Value, E> {
        Ok(Value::I32(i32::from(v)))
    }

    fn visit_i16<E: de::Error>(self, v: i16) -> Result<Value, E> {
        Ok(Value::I32(i32::from(v)))
    }

    fn visit_i32<E: de::Error>(self, v: i32) -> Result<Value, E> {
        Ok(Value::I32(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        // MMDB has no signed 64-bit type; accept values that fit int32, reject the rest.
        i32::try_from(v)
            .map(Value::I32)
            .map_err(|_| de::Error::custom("i64 outside int32 range has no MMDB representation"))
    }

    fn visit_u8<E: de::Error>(self, v: u8) -> Result<Value, E> {
        Ok(Value::U16(u16::from(v)))
    }

    fn visit_u16<E: de::Error>(self, v: u16) -> Result<Value, E> {
        Ok(Value::U16(v))
    }

    fn visit_u32<E: de::Error>(self, v: u32) -> Result<Value, E> {
        Ok(Value::U32(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        Ok(Value::U64(v))
    }

    fn visit_u128<E: de::Error>(self, v: u128) -> Result<Value, E> {
        Ok(Value::U128(v))
    }

    fn visit_f32<E: de::Error>(self, v: f32) -> Result<Value, E> {
        Ok(Value::Float(v))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Double(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Value, E> {
        Ok(Value::Bytes(v.to_vec()))
    }

    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Value, E> {
        Ok(Value::Bytes(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut entries = BTreeMap::new();
        while let Some((k, v)) = map.next_entry::<String, Value>()? {
            entries.insert(k, v);
        }
        Ok(Value::Map(entries))
    }
}
