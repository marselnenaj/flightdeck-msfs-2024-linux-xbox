// SPDX-License-Identifier: MIT
use serde_json::Value;
use std::{collections::BTreeMap, sync::OnceLock};
pub fn language(header: Option<&str>) -> &'static str {
    let Some(header) = header.filter(|h| !h.trim().is_empty()) else {
        return "de";
    };
    let mut best = (0_f32, "en");
    for item in header.chars().take(8192).collect::<String>().split(',') {
        let mut parts = item.trim().split(';');
        let tag = parts
            .next()
            .unwrap_or("")
            .split('-')
            .next()
            .unwrap_or("")
            .to_lowercase();
        let mut quality = 1_f32;
        let mut valid = true;
        for part in parts {
            let q = part
                .trim()
                .strip_prefix("q=")
                .and_then(|v| v.parse::<f32>().ok());
            match q {
                Some(q) if (0.0..=1.0).contains(&q) => quality = q,
                _ => valid = false,
            }
        }
        if valid && quality > best.0 && ["de", "en"].contains(&tag.as_str()) {
            best = (quality, if tag == "de" { "de" } else { "en" });
        }
    }
    best.1
}
pub fn text(value: &str, locale: &str) -> String {
    static CATALOG: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    if locale != "en" {
        return value.into();
    }
    CATALOG
        .get_or_init(|| serde_json::from_str(include_str!("catalog.json")).unwrap_or_default())
        .get(value)
        .cloned()
        .unwrap_or_else(|| value.into())
}
pub fn localize(value: &mut Value, locale: &str) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if [
                    "label",
                    "detail",
                    "message",
                    "error",
                    "prepare_unavailable_reason",
                    "install_unavailable_reason",
                    "unavailable_reason",
                ]
                .contains(&key.as_str())
                    && let Some(s) = value.as_str()
                {
                    *value = Value::String(text(s, locale));
                } else {
                    localize(value, locale);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                localize(item, locale);
            }
        }
        _ => {}
    }
}
