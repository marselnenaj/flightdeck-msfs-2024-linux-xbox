// SPDX-License-Identifier: MIT
//! Preserve object order when verifying existing Python support-draft hashes.
//! This is deliberately separate from the sorted maps used by save protocols.
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, SeqAccess, Visitor},
};
use std::fmt;
enum Ordered {
    Null,
    Bool(bool),
    Integer(String),
    Float(f64),
    String(String),
    Array(Vec<Ordered>),
    Object(Vec<(String, Ordered)>),
}
impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Ordered;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON value")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Ordered, E> {
                Ok(Ordered::Null)
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Ordered, E> {
                Ok(Ordered::Bool(v))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Ordered, E> {
                Ok(Ordered::Integer(v.to_string()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Ordered, E> {
                Ok(Ordered::Integer(v.to_string()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Ordered, E> {
                Ok(Ordered::Float(v))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Ordered, E> {
                Ok(Ordered::String(v.into()))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Ordered, E> {
                Ok(Ordered::String(v))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Ordered, A::Error> {
                let mut values = Vec::new();
                while let Some(v) = a.next_element()? {
                    values.push(v);
                }
                Ok(Ordered::Array(values))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Ordered, A::Error> {
                let mut values = Vec::new();
                while let Some((k, v)) = a.next_entry()? {
                    values.push((k, v));
                }
                Ok(Ordered::Object(values))
            }
        }
        d.deserialize_any(V)
    }
}
fn render(value: &Ordered, depth: usize, out: &mut String) -> serde_json::Result<()> {
    match value {
        Ordered::Null => out.push_str("null"),
        Ordered::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
        Ordered::Integer(v) => out.push_str(v),
        Ordered::Float(v) => {
            let value = format!("{v:?}");
            if let Some((significand, exponent)) = value.split_once('e') {
                out.push_str(significand);
                let exponent = exponent
                    .parse::<i32>()
                    .expect("finite formatted float exponent");
                out.push_str(&format!(
                    "e{}{number:02}",
                    if exponent < 0 { "-" } else { "+" },
                    number = exponent.unsigned_abs()
                ));
            } else {
                out.push_str(&value);
            }
        }
        Ordered::String(v) => out.push_str(&serde_json::to_string(v)?),
        Ordered::Array(values) => {
            out.push('[');
            for (i, v) in values.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                out.push_str(&"  ".repeat(depth + 1));
                render(v, depth + 1, out)?;
            }
            if !values.is_empty() {
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
            }
            out.push(']');
        }
        Ordered::Object(values) => {
            out.push('{');
            for (i, (key, v)) in values.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                out.push_str(&"  ".repeat(depth + 1));
                out.push_str(&serde_json::to_string(key)?);
                out.push_str(": ");
                render(v, depth + 1, out)?;
            }
            if !values.is_empty() {
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
            }
            out.push('}');
        }
    }
    Ok(())
}
pub fn encoded(value: &serde_json::Value) -> serde_json::Result<Vec<u8>> {
    let data = serde_json::to_vec(value)?;
    let ordered: Ordered = serde_json::from_slice(&data)?;
    let mut output = String::new();
    render(&ordered, 0, &mut output)?;
    output.push('\n');
    Ok(output.into_bytes())
}
pub fn report_bytes(draft: &[u8]) -> crate::Result<Vec<u8>> {
    let ordered: Ordered = serde_json::from_slice(draft)?;
    let Ordered::Object(entries) = ordered else {
        return Err(crate::Error::Invalid("Invalid support draft."));
    };
    let report = entries
        .iter()
        .find(|(key, _)| key == "report")
        .ok_or(crate::Error::Invalid("Invalid support draft."))?;
    let mut output = String::new();
    render(&report.1, 0, &mut output)?;
    output.push('\n');
    Ok(output.into_bytes())
}
