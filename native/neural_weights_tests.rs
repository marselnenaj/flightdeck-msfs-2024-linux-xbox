// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use super::*;
use std::fs;

#[test]
fn unknown_sources_and_existing_destinations_are_never_modified() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("model.bin");
    let output = temp.path().join("cache");
    fs::write(&source, b"unrecognized model").unwrap();
    assert!(import(&source, &output).is_err());
    assert!(!output.exists());
    fs::create_dir(&output).unwrap();
    fs::write(output.join("keep"), b"existing cache").unwrap();
    assert!(import(&source, &output).is_err());
    assert_eq!(fs::read(output.join("keep")).unwrap(), b"existing cache");
    assert_eq!(fs::read(&source).unwrap(), b"unrecognized model");
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
}

#[test]
fn raw_archive_provenance_cannot_masquerade_as_an_original_dll() {
    let lock: Value =
        serde_json::from_str(include_str!("../compat/neural-weights.lock.json")).unwrap();
    let tables = lock["tables"].clone();
    let manifest = source_manifest("weights-ht", ARCHIVE_SHA, tables, json!({}));
    assert!(neural_assets::validate_weight_manifest(&manifest).is_ok());
    for (key, bad) in [
        ("archive_sha256", json!(DLL_SHA)),
        ("schema", json!(neural_assets::WEIGHT_SCHEMA)),
        ("archive_records", json!(152)),
        ("nvidia_equivalence", json!(true)),
        ("converter", json!("unknown")),
    ] {
        let mut invalid = manifest.clone();
        invalid[key] = bad;
        assert!(
            neural_assets::validate_weight_manifest(&invalid).is_err(),
            "{key}"
        );
    }
    let mut invalid = manifest.clone();
    invalid["source"]["kind"] = json!("nr-dll");
    assert!(neural_assets::validate_weight_manifest(&invalid).is_err());
    invalid["source"]["sha256"] = json!(DLL_SHA);
    assert!(neural_assets::validate_weight_manifest(&invalid).is_ok());
    invalid["tables"]["block0-ffn.f32"]["sha256"] = json!("a".repeat(64));
    assert!(neural_assets::validate_weight_manifest(&invalid).is_err());
}

#[test]
#[ignore = "requires the exact local NR archive; writes a disposable 600 MiB cache"]
fn real_archive_import_matches_all_independent_upstream_table_hashes() {
    let source =
        std::env::var_os("FLIGHTDECK_TEST_NR_ARCHIVE").expect("FLIGHTDECK_TEST_NR_ARCHIVE");
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("weights");
    let result = import(Path::new(&source), &output).unwrap();
    assert_eq!(result["tables"], 223);
    assert!(neural_assets::weights(&output, None).is_ok());
    // Re-import must preserve the published cache, including its manifest.
    let manifest = fs::read(output.join("manifest.json")).unwrap();
    assert!(import(Path::new(&source), &output).is_err());
    assert_eq!(fs::read(output.join("manifest.json")).unwrap(), manifest);
    // A changed table is rejected even if the cache's self-reported hash changes.
    let table = output.join("post70-head.f32");
    let mut bytes = fs::read(&table).unwrap();
    bytes[0] ^= 1;
    fs::write(&table, &bytes).unwrap();
    let mut m: Value = serde_json::from_slice(&manifest).unwrap();
    m["tables"]["post70-head.f32"]["sha256"] = json!(files::sha256(&bytes));
    fs::write(
        output.join("manifest.json"),
        serde_json::to_vec(&m).unwrap(),
    )
    .unwrap();
    assert!(neural_assets::weights(&output, None).is_err());
}

#[test]
fn malformed_archive_headers_and_records_fail_without_panicking() {
    for n in 0..128 {
        let mut bytes = vec![0; n];
        if n >= 8 {
            bytes[..8].copy_from_slice(&(n as u64).to_le_bytes());
        }
        assert!(neural_decode::records(&bytes).is_err());
        if n >= 16 {
            bytes[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
            assert!(neural_decode::records(&bytes).is_err());
        }
    }
    assert_eq!(neural_decode::record_sizes().len(), 153);
}

#[test]
fn bridge_maps_match_pinned_hashes_and_are_inverse_permutations() {
    let forward = neural_decode::bridge(false);
    let backward = neural_decode::bridge(true);
    assert_eq!(
        files::sha256(&forward),
        "c942210afd8ffc8a2a1e4ed81df546e5e08d4c70e20f74fdddc0fe564f224ab8"
    );
    assert_eq!(
        files::sha256(&backward),
        "cb950400c76a6a1602ead35a817c851e552b5a301bc8e94561ed26785d1e2754"
    );
    for (i, v) in forward.as_chunks::<4>().0.iter().enumerate() {
        let index = u32::from_le_bytes(*v) as usize;
        assert_eq!(
            u32::from_le_bytes(backward[index * 4..index * 4 + 4].try_into().unwrap()) as usize,
            i
        );
    }
}

#[test]
fn half_conversion_covers_every_encoding_with_independent_f64_reference() {
    for bits in 0..=u16::MAX {
        let sign = if bits & 0x8000 == 0 { 1.0_f64 } else { -1.0 };
        let exponent = (bits >> 10) & 31;
        let mantissa = bits & 1023;
        let expected = match exponent {
            0 => sign * f64::from(mantissa) * 2_f64.powi(-24),
            31 if mantissa == 0 => sign * f64::INFINITY,
            31 => f64::NAN,
            _ => sign * (1.0 + f64::from(mantissa) / 1024.0) * 2_f64.powi(i32::from(exponent) - 15),
        } as f32;
        let actual = neural_decode::half(bits);
        if expected.is_nan() {
            assert!(actual.is_nan());
        } else {
            assert_eq!(actual.to_bits(), expected.to_bits(), "{bits:04x}");
        }
    }
}
