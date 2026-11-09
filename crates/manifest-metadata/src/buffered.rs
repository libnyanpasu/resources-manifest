//! A self-describing value buffered for a second deserialization pass.
//!
//! `serde_json::Value` keeps only the last of duplicate keys, which would hide
//! the duplicate-key errors of the types decoded from it. [`Buffered`] keeps
//! every entry in input order and replays them unchanged, so the second pass
//! reports the same errors as decoding the input directly.

use std::fmt;
use std::marker::PhantomData;

use serde::de::value::{MapDeserializer, SeqDeserializer, StringDeserializer};
use serde::de::{self, Deserialize, Deserializer, IntoDeserializer, MapAccess, SeqAccess, Visitor};

#[derive(Debug)]
pub(crate) enum Buffered {
    Unit,
    Bool(bool),
    U64(u64),
    I64(i64),
    F64(f64),
    String(String),
    Seq(Vec<Buffered>),
    Map(Vec<(Buffered, Buffered)>),
}

impl Buffered {
    /// Whether this is a map with an entry for `key`.
    pub(crate) fn has_key(&self, key: &str) -> bool {
        matches!(self, Self::Map(entries)
            if entries.iter().any(|(name, _)| matches!(name, Self::String(name) if name == key)))
    }
}

impl<'de> Deserialize<'de> for Buffered {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BufferedVisitor;

        impl<'de> Visitor<'de> for BufferedVisitor {
            type Value = Buffered;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("any JSON value")
            }
            fn visit_unit<E>(self) -> Result<Buffered, E> {
                Ok(Buffered::Unit)
            }
            fn visit_none<E>(self) -> Result<Buffered, E> {
                Ok(Buffered::Unit)
            }
            fn visit_some<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Buffered, D::Error> {
                Buffered::deserialize(deserializer)
            }
            fn visit_bool<E>(self, value: bool) -> Result<Buffered, E> {
                Ok(Buffered::Bool(value))
            }
            fn visit_u64<E>(self, value: u64) -> Result<Buffered, E> {
                Ok(Buffered::U64(value))
            }
            fn visit_i64<E>(self, value: i64) -> Result<Buffered, E> {
                Ok(Buffered::I64(value))
            }
            fn visit_f64<E>(self, value: f64) -> Result<Buffered, E> {
                Ok(Buffered::F64(value))
            }
            fn visit_str<E>(self, value: &str) -> Result<Buffered, E> {
                Ok(Buffered::String(value.to_owned()))
            }
            fn visit_string<E>(self, value: String) -> Result<Buffered, E> {
                Ok(Buffered::String(value))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Buffered, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(Buffered::Seq(items))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Buffered, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(Buffered::Map(entries))
            }
        }

        deserializer.deserialize_any(BufferedVisitor)
    }
}

/// Replays a [`Buffered`] value with the caller's error type.
pub(crate) struct BufferedDeserializer<E> {
    value: Buffered,
    error: PhantomData<E>,
}

impl<'de, E: de::Error> IntoDeserializer<'de, E> for Buffered {
    type Deserializer = BufferedDeserializer<E>;

    fn into_deserializer(self) -> Self::Deserializer {
        BufferedDeserializer {
            value: self,
            error: PhantomData,
        }
    }
}

impl<'de, E: de::Error> Deserializer<'de> for BufferedDeserializer<E> {
    type Error = E;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, E> {
        match self.value {
            Buffered::Unit => visitor.visit_unit(),
            Buffered::Bool(value) => visitor.visit_bool(value),
            Buffered::U64(value) => visitor.visit_u64(value),
            Buffered::I64(value) => visitor.visit_i64(value),
            Buffered::F64(value) => visitor.visit_f64(value),
            Buffered::String(value) => visitor.visit_string(value),
            // The deserializers, unlike their access halves, reject entries the
            // visitor leaves unread.
            Buffered::Seq(items) => {
                SeqDeserializer::new(items.into_iter()).deserialize_any(visitor)
            }
            Buffered::Map(entries) => {
                MapDeserializer::new(entries.into_iter()).deserialize_any(visitor)
            }
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, E> {
        match self.value {
            Buffered::Unit => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, E> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, E> {
        match self.value {
            // Unit variants are strings on the wire.
            Buffered::String(value) => {
                StringDeserializer::<E>::new(value).deserialize_enum(name, variants, visitor)
            }
            _ => self.deserialize_any(visitor),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map
        struct identifier ignored_any
    }
}
