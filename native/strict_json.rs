// SPDX-License-Identifier: MIT
//! JSON values with duplicate keys rejected at every nesting level.
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;
use std::fmt;

/// serde_json::Value normally accepts duplicate object keys. Protocol/journal
/// input must reject them at every level, including data not used by this version.
struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Strict;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a unique-key JSON value")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Strict, E> {
                serde_json::Number::from_f64(v)
                    .map(|v| Strict(Value::Number(v)))
                    .ok_or_else(|| E::custom("non-finite number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut v = Vec::new();
                while let Some(Strict(item)) = seq.next_element()? {
                    v.push(item);
                }
                Ok(Strict(Value::Array(v)))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut v = serde_json::Map::new();
                while let Some((key, Strict(value))) = map.next_entry::<String, Strict>()? {
                    if v.insert(key, value).is_some() {
                        return Err(de::Error::custom("duplicate key"));
                    }
                }
                Ok(Strict(Value::Object(v)))
            }
        }
        let raw = Box::<serde_json::value::RawValue>::deserialize(d)?;
        let mut parser = serde_json::Deserializer::from_str(raw.get());
        match raw.get().as_bytes().first() {
            Some(b'{') => parser.deserialize_map(V).map_err(de::Error::custom),
            Some(b'[') => parser.deserialize_seq(V).map_err(de::Error::custom),
            _ => {
                let value: Value = serde_json::from_str(raw.get()).map_err(de::Error::custom)?;
                if value
                    .as_number()
                    .is_some_and(|number| !number.as_f64().is_some_and(f64::is_finite))
                {
                    return Err(de::Error::custom("Non-finite JSON number"));
                }
                Ok(Strict(value))
            }
        }
    }
}
pub(crate) fn decode(bytes: &[u8]) -> serde_json::Result<Value> {
    // Inspect real object/array syntax through RawValue, so serde_json's
    // private number map tokens cannot be mistaken for actual object members.
    let Strict(value) = serde_json::from_slice(bytes)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numbers_remain_numbers_and_private_looking_objects_remain_objects() {
        let value=decode(br#"{"float":0.125,"integer":18446744073709551615,"literal":{"$serde_json::private::Number":"1.5"}}"#).expect("JSON");
        assert_eq!(value["float"].as_f64(), Some(0.125));
        assert_eq!(value["integer"].as_u64(), Some(u64::MAX));
        assert!(value["literal"].is_object());
        assert!(decode(br#"{"nested":{"a":1.5,"a":2.5}}"#).is_err());
    }
}
