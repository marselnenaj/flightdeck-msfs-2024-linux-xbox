// SPDX-License-Identifier: MIT
//! Frozen, synthetic pre-migration contracts. No Python application is needed.
#![allow(clippy::unwrap_used)]
use flightdeck::{files, framework, save_state};
use serde_json::Value;
use std::fs;

#[test]
fn legacy_saves_decode_reencode_and_reject_corruption() {
    let fixtures: Value =
        serde_json::from_str(include_str!("fixtures/legacy-python/saves.json")).unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let raw = hex::decode(case["hex"].as_str().unwrap()).unwrap();
        let state = save_state::decode(&raw).unwrap();
        assert_eq!(save_state::encode(&state).unwrap(), raw);
        assert_eq!(files::sha256(&raw), case["canonical_sha256"]);
        assert_eq!(
            save_state::content_digest(&state).unwrap(),
            case["content_sha256"]
        );
        assert_eq!(
            state.containers.len(),
            case["containers"].as_u64().unwrap() as usize
        );
        assert_eq!(
            state
                .containers
                .values()
                .map(|c| c.blobs.len())
                .sum::<usize>(),
            case["blobs"].as_u64().unwrap() as usize
        );
        for invalid in [&raw[..raw.len() - 1], &raw[..20]] {
            assert!(save_state::decode(invalid).is_err());
        }
        let mut trailing = raw;
        trailing.push(b'x');
        assert!(save_state::decode(&trailing).is_err());
    }
}

#[test]
fn legacy_framework_registry_views_and_case_insensitive_files_agree() {
    let fixtures: Value =
        serde_json::from_str(include_str!("fixtures/legacy-python/framework.json")).unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("system.reg"),
            case["registry"].as_str().unwrap(),
        )
        .unwrap();
        for relative in case["clr_files"].as_array().unwrap() {
            let path = dir.path().join(relative.as_str().unwrap());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"MZfixture").unwrap();
        }
        assert_eq!(
            serde_json::to_value(framework::status(dir.path())).unwrap(),
            case["expected"]
        );
    }
}
