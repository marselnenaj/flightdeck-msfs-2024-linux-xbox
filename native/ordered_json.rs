// SPDX-License-Identifier: MIT
//! Preserve object order when verifying existing Python support-draft hashes.
//! Object order and arbitrary integer digits survive the shared engine's JSON
//! features; floating-point spellings still follow the old Python encoder.
enum Ordered {
    Null,
    Bool(bool),
    Integer(String),
    Float(f64),
    String(String),
    Array(Vec<Ordered>),
    Object(Vec<(String, Ordered)>),
}
impl Ordered {
    fn from_value(value: serde_json::Value) -> serde_json::Result<Self> {
        use serde_json::Value;
        Ok(match value {
            Value::Null => Self::Null,
            Value::Bool(v) => Self::Bool(v),
            Value::String(v) => Self::String(v),
            Value::Number(v) => {
                let raw = v.to_string();
                if raw.contains(['.', 'e', 'E']) {
                    Self::Float(v.as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                        serde_json::Error::io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Non-finite support report number",
                        ))
                    })?)
                } else {
                    Self::Integer(raw)
                }
            }
            Value::Array(values) => Self::Array(
                values
                    .into_iter()
                    .map(Self::from_value)
                    .collect::<serde_json::Result<_>>()?,
            ),
            Value::Object(values) => Self::Object(
                values
                    .into_iter()
                    .map(|(key, value)| Ok((key, Self::from_value(value)?)))
                    .collect::<serde_json::Result<_>>()?,
            ),
        })
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
    let ordered = Ordered::from_value(value.clone())?;
    let mut output = String::new();
    render(&ordered, 0, &mut output)?;
    output.push('\n');
    Ok(output.into_bytes())
}
pub fn report_bytes(draft: &[u8]) -> crate::Result<Vec<u8>> {
    let ordered = Ordered::from_value(crate::strict_json::decode(draft)?)?;
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
