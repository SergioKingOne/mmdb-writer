//! A `serde::Serializer` that projects any `Serialize` value into a [`Value`], gated on the
//! `serde` feature.
//!
//! MMDB has no null type, so the serializer's `Ok` type is `Option<Value>`: `None` means
//! *"the parent map/struct should drop this entry"*. Only `Option::None`, `()`, and unit
//! structs produce `None`.

use std::collections::BTreeMap;

use serde::Serialize;
use serde::ser::{
    self, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};

use crate::error::Error;
use crate::value::Value;

impl ser::Error for Error {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Error::Serialize(msg.to_string())
    }
}

/// Serialize any `Serialize` value into an MMDB [`Value`].
pub(crate) fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value, Error> {
    value
        .serialize(ValueSerializer)?
        .ok_or(Error::UnsupportedValue("top-level null / Option::None"))
}

struct ValueSerializer;

/// `None` = "drop this entry" (used by map/struct parents).
type Out = Option<Value>;

impl ser::Serializer for ValueSerializer {
    type Ok = Out;
    type Error = Error;

    type SerializeSeq = SeqSer;
    type SerializeTuple = SeqSer;
    type SerializeTupleStruct = SeqSer;
    type SerializeTupleVariant = TupleVariantSer;
    type SerializeMap = MapSer;
    type SerializeStruct = MapSer;
    type SerializeStructVariant = StructVariantSer;

    fn serialize_bool(self, v: bool) -> Result<Out, Error> {
        Ok(Some(Value::Bool(v)))
    }

    fn serialize_i8(self, v: i8) -> Result<Out, Error> {
        Ok(Some(Value::I32(i32::from(v))))
    }

    fn serialize_i16(self, v: i16) -> Result<Out, Error> {
        Ok(Some(Value::I32(i32::from(v))))
    }

    fn serialize_i32(self, v: i32) -> Result<Out, Error> {
        Ok(Some(Value::I32(v)))
    }

    fn serialize_i64(self, _v: i64) -> Result<Out, Error> {
        Err(Error::UnsupportedValue("i64"))
    }

    fn serialize_u8(self, v: u8) -> Result<Out, Error> {
        Ok(Some(Value::U16(u16::from(v))))
    }

    fn serialize_u16(self, v: u16) -> Result<Out, Error> {
        Ok(Some(Value::U16(v)))
    }

    fn serialize_u32(self, v: u32) -> Result<Out, Error> {
        Ok(Some(Value::U32(v)))
    }

    fn serialize_u64(self, v: u64) -> Result<Out, Error> {
        Ok(Some(Value::U64(v)))
    }

    fn serialize_u128(self, v: u128) -> Result<Out, Error> {
        Ok(Some(Value::U128(v)))
    }

    fn serialize_f32(self, v: f32) -> Result<Out, Error> {
        Ok(Some(Value::Float(v)))
    }

    fn serialize_f64(self, v: f64) -> Result<Out, Error> {
        Ok(Some(Value::Double(v)))
    }

    fn serialize_char(self, v: char) -> Result<Out, Error> {
        Ok(Some(Value::String(v.to_string())))
    }

    fn serialize_str(self, v: &str) -> Result<Out, Error> {
        Ok(Some(Value::String(v.to_owned())))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Out, Error> {
        Ok(Some(Value::Bytes(v.to_vec())))
    }

    fn serialize_none(self) -> Result<Out, Error> {
        Ok(None)
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Out, Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Out, Error> {
        Ok(None)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Out, Error> {
        Ok(None)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Out, Error> {
        Ok(Some(Value::String(variant.to_owned())))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Out, Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Out, Error> {
        // Externally tagged: {"variant": value}.
        let mut map = BTreeMap::new();
        if let Some(v) = value.serialize(ValueSerializer)? {
            map.insert(variant.to_owned(), v);
        }
        Ok(Some(Value::Map(map)))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<SeqSer, Error> {
        Ok(SeqSer {
            items: Vec::with_capacity(len.unwrap_or(0)),
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<SeqSer, Error> {
        Ok(SeqSer {
            items: Vec::with_capacity(len),
        })
    }

    fn serialize_tuple_struct(self, _name: &'static str, len: usize) -> Result<SeqSer, Error> {
        Ok(SeqSer {
            items: Vec::with_capacity(len),
        })
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<TupleVariantSer, Error> {
        Ok(TupleVariantSer {
            variant,
            items: Vec::with_capacity(len),
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<MapSer, Error> {
        Ok(MapSer {
            entries: BTreeMap::new(),
            pending_key: None,
        })
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<MapSer, Error> {
        Ok(MapSer {
            entries: BTreeMap::new(),
            pending_key: None,
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<StructVariantSer, Error> {
        Ok(StructVariantSer {
            variant,
            entries: BTreeMap::new(),
        })
    }
}

pub(crate) struct SeqSer {
    items: Vec<Value>,
}

impl SerializeSeq for SeqSer {
    type Ok = Out;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        if let Some(v) = value.serialize(ValueSerializer)? {
            self.items.push(v);
        }
        Ok(())
    }

    fn end(self) -> Result<Out, Error> {
        Ok(Some(Value::Array(self.items)))
    }
}

impl SerializeTuple for SeqSer {
    type Ok = Out;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Out, Error> {
        SerializeSeq::end(self)
    }
}

impl SerializeTupleStruct for SeqSer {
    type Ok = Out;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Out, Error> {
        SerializeSeq::end(self)
    }
}

pub(crate) struct TupleVariantSer {
    variant: &'static str,
    items: Vec<Value>,
}

impl SerializeTupleVariant for TupleVariantSer {
    type Ok = Out;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        if let Some(v) = value.serialize(ValueSerializer)? {
            self.items.push(v);
        }
        Ok(())
    }

    fn end(self) -> Result<Out, Error> {
        let mut map = BTreeMap::new();
        map.insert(self.variant.to_owned(), Value::Array(self.items));
        Ok(Some(Value::Map(map)))
    }
}

pub(crate) struct MapSer {
    entries: BTreeMap<String, Value>,
    pending_key: Option<String>,
}

impl SerializeMap for MapSer {
    type Ok = Out;
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Error> {
        let key_value = key
            .serialize(ValueSerializer)?
            .ok_or(Error::UnsupportedValue("null map key"))?;
        let Value::String(k) = key_value else {
            return Err(Error::UnsupportedValue("non-string map key"));
        };
        self.pending_key = Some(k);
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        let key = self.pending_key.take().ok_or(Error::UnsupportedValue(
            "map value serialized before its key",
        ))?;
        if let Some(v) = value.serialize(ValueSerializer)? {
            self.entries.insert(key, v);
        }
        // `None` → drop: this is how `Option::None` map values are skipped.
        Ok(())
    }

    fn end(self) -> Result<Out, Error> {
        Ok(Some(Value::Map(self.entries)))
    }
}

impl SerializeStruct for MapSer {
    type Ok = Out;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        if let Some(v) = value.serialize(ValueSerializer)? {
            self.entries.insert(key.to_owned(), v);
        }
        Ok(())
    }

    fn end(self) -> Result<Out, Error> {
        Ok(Some(Value::Map(self.entries)))
    }
}

pub(crate) struct StructVariantSer {
    variant: &'static str,
    entries: BTreeMap<String, Value>,
}

impl SerializeStructVariant for StructVariantSer {
    type Ok = Out;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        if let Some(v) = value.serialize(ValueSerializer)? {
            self.entries.insert(key.to_owned(), v);
        }
        Ok(())
    }

    fn end(self) -> Result<Out, Error> {
        let mut outer = BTreeMap::new();
        outer.insert(self.variant.to_owned(), Value::Map(self.entries));
        Ok(Some(Value::Map(outer)))
    }
}
