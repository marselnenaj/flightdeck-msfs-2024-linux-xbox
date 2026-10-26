// SPDX-License-Identifier: MIT
//! Bounded file access, hashing and strict JSON shared by every tool.
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

/// A failed check or a refused input; the message is shown as is.
#[derive(Debug)]
pub struct Failure(pub String);
pub type Result<T> = std::result::Result<T, Failure>;

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Failure {}
impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}
impl From<serde_json::Error> for Failure {
    fn from(error: serde_json::Error) -> Self {
        Self(error.to_string())
    }
}
pub fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Failure(message.into()))
}
/// Fails with the source location and the text of a condition that is false.
#[macro_export]
macro_rules! ensure {
    ($condition:expr) => {
        $crate::util::require(
            $condition,
            format!("Check failed at {}:{}: {}", file!(), line!(), stringify!($condition)),
        )?
    };
    ($condition:expr, $($detail:tt)+) => {
        $crate::util::require(
            $condition,
            format!("Check failed at {}:{}: {}: {}", file!(), line!(), stringify!($condition), format!($($detail)+)),
        )?
    };
}

pub fn require(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition { Ok(()) } else { fail(message) }
}

/// The checkout this tool was built from, or the current directory when the
/// executable runs somewhere that checkout does not exist.
pub fn root() -> PathBuf {
    let built = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools directory inside the checkout");
    if built.join("Cargo.toml").is_file() {
        return built.to_path_buf();
    }
    std::env::current_dir().unwrap_or_else(|_| built.to_path_buf())
}

pub fn digest(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// Reads a regular file of at most `maximum` bytes without following a link.
/// O_NONBLOCK keeps a supplied FIFO from blocking before it is rejected.
pub fn read_regular(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    use rustix::fs::{Mode, OFlags};
    let descriptor = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| Failure(format!("{}: {error}", path.display())))?;
    let file = File::from(descriptor);
    let info = file.metadata()?;
    require(
        info.is_file() && info.len() <= maximum,
        "Input is not a regular file within the size limit",
    )?;
    let mut data = Vec::new();
    file.take(maximum + 1).read_to_end(&mut data)?;
    require(data.len() as u64 <= maximum, "Input exceeds the size limit")?;
    Ok(data)
}

pub const JSON_MAX: u64 = 1024 * 1024;

/// A JSON object without duplicate keys at any depth. serde_json keeps the
/// last duplicate silently, which would let two readers disagree on a file.
pub fn strict_json(data: &[u8], label: &str, maximum: u64) -> Result<Map<String, Value>> {
    require(
        data.len() as u64 <= maximum,
        format!("{label} exceeds the JSON size limit"),
    )?;
    let invalid = || Failure(format!("{label} is invalid JSON"));
    let text = std::str::from_utf8(data).map_err(|_| invalid())?;
    let Strict(value) = serde_json::from_str(text).map_err(|_| invalid())?;
    match value {
        Value::Object(map) => Ok(map),
        _ => fail(format!("{label} must be an object")),
    }
}

struct Strict(Value);
impl<'de> serde::Deserialize<'de> for Strict {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Strict;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON value")
            }
            fn visit_bool<E>(self, v: bool) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::Bool(v)))
            }
            fn visit_i64<E>(self, v: i64) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_u64<E>(self, v: u64) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Strict, E> {
                serde_json::Number::from_f64(v)
                    .map(|v| Strict(Value::Number(v)))
                    .ok_or_else(|| E::custom("Non-finite JSON number"))
            }
            fn visit_str<E>(self, v: &str) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::String(v.into())))
            }
            fn visit_unit<E>(self) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element::<Strict>()? {
                    items.push(item.0);
                }
                Ok(Strict(Value::Array(items)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut items = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    let value = map.next_value::<Strict>()?;
                    if items.insert(key, value.0).is_some() {
                        return Err(serde::de::Error::custom("Duplicate JSON key"));
                    }
                }
                Ok(Strict(Value::Object(items)))
            }
        }
        use serde::Deserializer;
        let raw = Box::<serde_json::value::RawValue>::deserialize(deserializer)?;
        let mut parser = serde_json::Deserializer::from_str(raw.get());
        match raw.get().as_bytes().first() {
            Some(b'{') => parser
                .deserialize_map(Visitor)
                .map_err(serde::de::Error::custom),
            Some(b'[') => parser
                .deserialize_seq(Visitor)
                .map_err(serde::de::Error::custom),
            _ => {
                let value: Value =
                    serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
                if value
                    .as_number()
                    .is_some_and(|number| !number.as_f64().is_some_and(f64::is_finite))
                {
                    return Err(serde::de::Error::custom("Non-finite JSON number"));
                }
                Ok(Strict(value))
            }
        }
    }
}

/// Two-space indented JSON with a final newline, keys in sorted order.
pub fn pretty(value: &Value) -> Vec<u8> {
    let mut value = value.clone();
    value.sort_all_objects();
    let mut text = serde_json::to_string_pretty(&value).expect("JSON value");
    text.push('\n');
    text.into_bytes()
}

/// A canonical relative archive path: no empty, `.` or `..` component, no
/// backslash, drive separator or control character.
pub fn safe_name(name: &str) -> Result<&str> {
    require(
        !name.is_empty()
            && name.len() <= 1024
            && !name.contains(['\\', ':'])
            && !name.chars().any(|c| (c as u32) < 32 || c as u32 == 127)
            && !name.starts_with('/')
            && !name.split('/').any(|part| matches!(part, "" | "." | "..")),
        "Archive path must be a unique canonical relative file path",
    )?;
    Ok(name)
}

/// The launcher version from the workspace manifest.
pub fn version(root: &Path) -> Result<String> {
    let manifest = String::from_utf8(read_regular(&root.join("Cargo.toml"), JSON_MAX)?)
        .map_err(|_| Failure("Cargo.toml is not UTF-8".into()))?;
    let package = manifest
        .split_once("[package]")
        .map(|(_, rest)| rest.split("\n[").next().unwrap_or(rest))
        .ok_or_else(|| Failure("Cargo.toml has no package section".into()))?;
    package
        .lines()
        .find_map(|line| {
            let value = line
                .strip_prefix("version")?
                .trim_start()
                .strip_prefix('=')?;
            Some(value.trim().trim_matches('"').to_string())
        })
        .ok_or_else(|| Failure("Cargo.toml has no package version".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_json_rejects_duplicates_non_objects_and_oversized_input() {
        assert_eq!(
            strict_json(br#"{"a":{"b":[1,{"c":null}]}}"#, "Fixture", 64).expect("valid")["a"]["b"]
                [1]["c"],
            Value::Null
        );
        for text in [
            r#"{"a":1,"a":2}"#,
            r#"{"a":{"b":1,"b":1}}"#,
            r#"{"a":[{"b":1,"b":2}]}"#,
            r#"{"a":NaN}"#,
            r#"{"a":1e999}"#,
            "{",
        ] {
            let error = strict_json(text.as_bytes(), "Fixture", 64).expect_err(text);
            assert_eq!(error.0, "Fixture is invalid JSON", "{text}");
        }
        assert_eq!(
            strict_json(b"[]", "Fixture", 64).expect_err("array").0,
            "Fixture must be an object"
        );
        assert_eq!(
            strict_json(b"{}  ", "Fixture", 3).expect_err("size").0,
            "Fixture exceeds the JSON size limit"
        );
    }

    #[test]
    fn archive_names_must_be_canonical_relative_paths() {
        for name in ["a", "a/b.txt", "flightdeck-linux/bin/flightdeck"] {
            assert!(safe_name(name).is_ok(), "{name}");
        }
        for name in [
            "", "/a", "a//b", "a/./b", "a/../b", "..", "a\\b", "c:a", "a\nb", "a/", "a\x7f",
        ] {
            assert!(safe_name(name).is_err(), "{name:?}");
        }
        assert!(safe_name(&"a".repeat(1025)).is_err());
    }

    #[test]
    fn regular_files_are_bounded_and_links_and_fifos_refused() {
        let temp = tempfile::tempdir().expect("temp");
        let file = temp.path().join("file");
        std::fs::write(&file, b"12345").expect("write");
        assert_eq!(read_regular(&file, 5).expect("read"), b"12345");
        assert!(read_regular(&file, 4).is_err());
        std::os::unix::fs::symlink(&file, temp.path().join("link")).expect("link");
        assert!(read_regular(&temp.path().join("link"), 5).is_err());
        rustix::fs::mknodat(
            rustix::fs::CWD,
            temp.path().join("fifo"),
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
            0,
        )
        .expect("fifo");
        assert!(read_regular(&temp.path().join("fifo"), 5).is_err());
        assert!(read_regular(temp.path(), 5).is_err());
    }

    #[test]
    fn version_is_read_from_the_package_section_only() {
        let temp = tempfile::tempdir().expect("temp");
        std::fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"1.2.3\"\n\n[dependencies]\nversion = \"9\"\n",
        )
        .expect("write");
        assert_eq!(version(temp.path()).expect("version"), "1.2.3");
        assert_eq!(version(&root()).expect("checkout").split('.').count(), 3);
    }
}
