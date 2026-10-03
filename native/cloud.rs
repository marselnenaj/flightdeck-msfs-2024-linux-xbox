// SPDX-License-Identifier: MIT
//! Shared cloud protocol boundaries. Credentials remain in the native Store helper.
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;
use std::{
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, Failure>;

/// Contains only static codes and bounded diagnostics, never remote text or identity data.
#[derive(Clone, Debug)]
pub struct Failure {
    pub code: &'static str,
    pub committed_containers: usize,
    pub recovery_required: bool,
    pub http_status: Option<u16>,
    pub native_hresult: Option<u32>,
}
impl Failure {
    pub fn new(code: &'static str) -> Self {
        Self {
            code,
            committed_containers: 0,
            recovery_required: false,
            http_status: None,
            native_hresult: None,
        }
    }
    pub fn status(mut self, status: u16) -> Self {
        self.http_status = (100..=599).contains(&status).then_some(status);
        self
    }
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.code {
            "authentication" => "Die Xbox-Anmeldung muss erneuert werden.",
            "changed" | "local_changed" => "Die Spielstände haben sich geändert. Bitte erneut vergleichen.",
            "conflict" => "Die Spielstände unterscheiden sich. Bitte einen Stand auswählen.",
            "invalid_scope" => "Das Xbox-Spielprofil hat sich geändert. Bitte erneut vergleichen.",
            "busy" => "Die Spielstände werden gerade verwendet. Bitte den Simulator beenden.",
            "lease_lost" => "Die Xbox-Cloud-Sperre ist nicht mehr verfügbar. Bitte später erneut vergleichen.",
            "quota" => "Der verfügbare Xbox-Cloud-Speicher reicht für diese Spielstände nicht aus.",
            "cancelled" => "Der Cloud-Vorgang wurde abgebrochen.",
            "deadline" => "Der Cloud-Vorgang hat sein Zeitlimit überschritten.",
            "unsafe_session" => "Die vorherige Spielsitzung wurde unterbrochen. Bitte Linux neu starten.",
            "invalid_snapshot" => "Die Cloud-Kopie ist ungültig. Bitte erneut herunterladen.",
            "invalid_plan" => "Dieser Spielstandvergleich ist nicht mehr gültig. Bitte erneut vergleichen.",
            "local_storage" | "durability_unknown" => "Die Spielstandsicherung konnte nicht sicher abgeschlossen werden.",
            _ => "Der Cloud-Abgleich konnte nicht bestätigt werden. Lokale Sicherungen bleiben erhalten.",
        })
    }
}
impl std::error::Error for Failure {}
impl From<std::io::Error> for Failure {
    fn from(_: std::io::Error) -> Self {
        Self::new("local_storage")
    }
}
impl From<rustix::io::Errno> for Failure {
    fn from(_: rustix::io::Errno) -> Self {
        Self::new("local_storage")
    }
}
impl From<serde_json::Error> for Failure {
    fn from(_: serde_json::Error) -> Self {
        Self::new("invalid_response")
    }
}
impl From<crate::Error> for Failure {
    fn from(e: crate::Error) -> Self {
        match e {
            crate::Error::Cloud(e) => e,
            crate::Error::Cancelled => Self::new("cancelled"),
            _ => Self::new("local_storage"),
        }
    }
}
pub fn require(ok: bool, code: &'static str) -> Result<()> {
    if ok { Ok(()) } else { Err(Failure::new(code)) }
}
pub fn check(cancel: &AtomicBool) -> Result<()> {
    require(!cancel.load(Ordering::Acquire), "cancelled")
}
pub fn remaining(cancel: &AtomicBool, deadline: Instant) -> Result<Duration> {
    check(cancel)?;
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| Failure::new("deadline"))
}

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
        d.deserialize_any(V)
    }
}
pub fn json(bytes: &[u8]) -> Result<Value> {
    let Strict(value) = serde_json::from_slice(bytes)?;
    require(value.is_object(), "invalid_response")?;
    Ok(value)
}
pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    Ok(serde_json::from_value(json(bytes)?)?)
}
pub fn text(value: &Value, maximum: usize, empty: bool) -> Result<&str> {
    let v = value
        .as_str()
        .ok_or_else(|| Failure::new("invalid_response"))?;
    require(
        (empty || !v.is_empty()) && !v.chars().any(|c| c < ' ' || c == '\u{7f}'),
        "invalid_response",
    )?;
    require(v.len() <= maximum, "bounds")?;
    Ok(v)
}
pub fn number(value: &Value, maximum: u64) -> Result<u64> {
    value
        .as_u64()
        .filter(|v| *v <= maximum)
        .ok_or_else(|| Failure::new("invalid_response"))
}
pub fn guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
pub fn identifier(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|v| {
        v.len() == 32
            && v.bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    })
}
