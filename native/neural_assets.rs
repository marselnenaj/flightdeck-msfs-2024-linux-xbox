// SPDX-License-Identifier: MIT
//! Version-bound local inputs for the experimental Linux neural renderer.
use crate::{Result, error::require, files, neural_weights};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

pub const WEIGHT_SCHEMA: &str = "dlss5-15799b1-amd-consumer-derived-v1";
const MAPS: [(&str, &str); 2] = [
    (
        "hwc-to-vit.i32",
        "c942210afd8ffc8a2a1e4ed81df546e5e08d4c70e20f74fdddc0fe564f224ab8",
    ),
    (
        "vit-to-hwc.i32",
        "cb950400c76a6a1602ead35a817c851e552b5a301bc8e94561ed26785d1e2754",
    ),
];

pub fn bundle(root: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let lock: Value = serde_json::from_str(include_str!("../compat/neural-rendering.lock.json"))?;
    let directory = files::directory(root, false)?;
    let mut result = BTreeMap::new();
    for (name, entry) in lock["files"]
        .as_object()
        .expect("embedded neural bundle lock")
    {
        let data = files::read_file(files::beneath(&directory, name)?, 32 * 1024 * 1024)?;
        let digest = files::sha256(&data);
        let matches = |entry: &Value| entry["bytes"] == data.len() && entry["sha256"] == digest;
        require(
            matches(entry) || (name == "bin/libdlss5_hip.so" && matches(&lock["rdna2"])),
            "Use the pinned neural-rendering bundle with either its original RDNA4 library or Flightdeck's verified gfx1030 build; a component is missing or changed.",
        )?;
        result.insert(name.clone(), data);
    }
    Ok(result)
}

/// A hardware whitelist alone cannot make an RDNA4 code object run on RDNA2.
/// Match the actual, already hash-validated native library to its target family.
pub fn check_architecture(bundle: &BTreeMap<String, Vec<u8>>, arch: &str) -> Result<()> {
    let digest = bundle.get("bin/libdlss5_hip.so").map(|b| files::sha256(b));
    require(
        digest.as_deref().is_some_and(|v| backend_matches(v, arch)),
        "The neural backend does not match this GPU. RX 6800/6900 require Flightdeck's gfx1030 bundle; RX 9060/9070 require the original RDNA4 bundle.",
    )
}

fn backend_matches(digest: &str, arch: &str) -> bool {
    let lock: Value = serde_json::from_str(include_str!("../compat/neural-rendering.lock.json"))
        .expect("embedded neural bundle lock");
    let expected = if arch == "gfx1030" {
        &lock["rdna2"]["sha256"]
    } else {
        &lock["files"]["bin/libdlss5_hip.so"]["sha256"]
    };
    ["gfx1030", "gfx1200", "gfx1201"].contains(&arch) && expected == digest
}

#[test]
fn native_code_must_match_the_gpu_architecture() {
    let lock: Value = serde_json::from_str(include_str!("../compat/neural-rendering.lock.json"))
        .expect("embedded lock");
    let rdna2 = lock["rdna2"]["sha256"].as_str().expect("RDNA2 digest");
    let rdna4 = lock["files"]["bin/libdlss5_hip.so"]["sha256"]
        .as_str()
        .expect("RDNA4 digest");
    assert!(files::hex_digest(rdna2) && files::hex_digest(rdna4) && rdna2 != rdna4);
    for arch in ["gfx1030", "gfx1200", "gfx1201", "gfx1100", "", "gfx1031"] {
        assert_eq!(backend_matches(rdna2, arch), arch == "gfx1030");
        assert_eq!(
            backend_matches(rdna4, arch),
            ["gfx1200", "gfx1201"].contains(&arch)
        );
        assert!(!backend_matches(&"0".repeat(64), arch));
    }
    assert!(check_architecture(&BTreeMap::new(), "gfx1030").is_err());
}

// File/shape contract of the pinned upstream converter, not model coefficients.
// guentra/dlss5-amd-hip-linux, linux/dlssnr/convert_dll.py::expected_tables.
pub fn table_sizes() -> BTreeMap<String, usize> {
    let mut sizes: BTreeMap<_, _> = [
        ("block0-mix.audit.f32", 512),
        ("post70-head.f32", 96),
        ("post70-scales.f32", 64),
        ("head-matrix.f32", 524288),
        ("decoder39-weights.f32", 524800),
    ]
    .into_iter()
    .map(|(n, s)| (n.to_owned(), s))
    .collect();
    for b in (5..23).chain(48..66) {
        let ch = if !(9..62).contains(&b) {
            64
        } else if !(15..56).contains(&b) {
            128
        } else {
            256
        };
        sizes.insert(format!("block{b}-ffn.f32"), 9 * ch * ch + ch);
        sizes.insert(
            format!("block{b}-attention-components.audit.f32"),
            4 * ch * ch + ch / 32 * 4096 + ch / 32,
        );
        sizes.insert(
            format!("block{b}-attention.f32"),
            4 * ch * ch + ch / 32 * 4096 + ch / 32 + ch,
        );
    }
    for (b, ch) in [(8, 64), (14, 128), (22, 256)] {
        sizes.insert(format!("block{b}-ds.f32"), 2 * ch * ch);
    }
    for b in (23..31).chain(40..48) {
        sizes.insert(format!("block{b}-ffwd.f32"), 524288);
        sizes.insert(format!("block{b}-ffwd-projection.f32"), 262656);
        sizes.insert(format!("block{b}-attention.f32"), 1114640);
    }
    for b in 31..39 {
        for (suffix, n) in [
            ("expand", 4194304),
            ("contract", 4195328),
            ("qkv", 3145760),
            ("projection", 1049600),
        ] {
            sizes.insert(format!("block{b}-{suffix}.f32"), n);
        }
    }
    for (b, ch) in [(48, 256), (56, 128), (62, 64), (66, 32)] {
        sizes.insert(format!("block{b}-weights.f32"), 2 * ch * ch + ch);
    }
    for b in (0..5).chain(66..71) {
        let prefix = if b == 70 {
            "post70".into()
        } else {
            format!("block{b}")
        };
        sizes.insert(format!("{prefix}-ffn.f32"), 8736);
        sizes.insert(format!("{prefix}-attention.f32"), 8225);
    }
    sizes.insert("block4-ds.f32".into(), 2048);
    for (name, _) in MAPS {
        sizes.insert(name.into(), 655360);
    }
    sizes
}

pub fn validate_weight_manifest(value: &Value) -> Result<BTreeMap<String, usize>> {
    let sizes = table_sizes();
    let lock: Value = serde_json::from_str(include_str!("../compat/neural-weights.lock.json"))
        .expect("embedded neural coefficient lock");
    require(
        ((value["schema"] == WEIGHT_SCHEMA && value["source_sha256"] == neural_weights::DLL_SHA)
            || neural_weights::validate_source(value))
            && value["upstream_commit"] == "15799b1600d57b849597a44be53ac892b7a2faea"
            && value["layout_mode"] == "amd-consumer-derived"
            && value["runtime_ready"] == true
            && value["experimental_runtime_ready"] == true
            && value["nvidia_equivalence"] == false
            && value["tables"]
                .as_object()
                .is_some_and(|v| v.len() == sizes.len())
            && sizes.iter().all(|(n, s)| {
                value["tables"][n]["count"] == *s
                    && lock["tables"][n]["count"] == *s
                    && value["tables"][n]["sha256"] == lock["tables"][n]["sha256"]
            }),
        "The converted neural weights are incomplete or use an unsupported layout. Import your local NR model with 'neural-weights'.",
    )?;
    Ok(sizes)
}

pub fn weights(root: &Path, expected: Option<&str>) -> Result<String> {
    let directory = files::directory(root, false)?;
    let bytes = files::read_file(files::beneath(&directory, "manifest.json")?, 1024 * 1024)?;
    let hash = files::sha256(&bytes);
    require(
        expected.is_none_or(|v| v == hash),
        "The neural weight manifest changed. Reconfigure the experimental profile.",
    )?;
    let manifest: Value = serde_json::from_slice(&bytes)?;
    let sizes = validate_weight_manifest(&manifest)?;
    let marker = files::read_file(files::beneath(&directory, "full-network.ok")?, 1024)?;
    require(
        marker == format!("{}\n", manifest["schema"].as_str().unwrap_or("")).as_bytes(),
        "The neural weight cache is not complete.",
    )?;
    for (name, count) in sizes {
        let data = files::read_file(files::beneath(&directory, &name)?, count * 4)?;
        let digest = files::sha256(&data);
        require(
            data.len() == count * 4 && manifest["tables"][&name]["sha256"] == digest,
            "A neural weight table is missing, truncated or changed.",
        )?;
        if let Some((_, expected)) = MAPS.iter().find(|(n, _)| *n == name) {
            require(
                digest == *expected,
                "The neural weight cache has an invalid bridge map.",
            )?;
        } else {
            require(
                data.as_chunks::<4>()
                    .0
                    .iter()
                    .all(|v| f32::from_le_bytes(*v).is_finite()),
                "The neural weight cache contains non-finite values.",
            )?;
        }
    }
    Ok(hash)
}

/// Check the PE64 export table, not arbitrary string occurrences in the image.
pub fn exports(data: &[u8], wanted: &[u8]) -> bool {
    fn word(d: &[u8], o: usize) -> Option<u16> {
        Some(u16::from_le_bytes(
            d.get(o..o.checked_add(2)?)?.try_into().ok()?,
        ))
    }
    fn dword(d: &[u8], o: usize) -> Option<usize> {
        Some(u32::from_le_bytes(d.get(o..o.checked_add(4)?)?.try_into().ok()?) as usize)
    }
    (|| -> Option<bool> {
        if data.get(..2)? != b"MZ" {
            return None;
        }
        let pe = dword(data, 60)?;
        if pe > 1024 * 1024
            || data.get(pe..pe + 4)? != b"PE\0\0"
            || word(data, pe + 4)? != 0x8664
            || word(data, pe + 24)? != 0x20b
        {
            return None;
        }
        let count = word(data, pe + 6)? as usize;
        let optional = word(data, pe + 20)? as usize;
        if count == 0 || count > 96 || optional < 120 || dword(data, pe + 24 + 108)? == 0 {
            return None;
        }
        let section = pe + 24 + optional;
        let offset = |rva: usize| -> Option<usize> {
            for i in 0..count {
                let s = section + i * 40;
                let start = dword(data, s + 12)?;
                let size = dword(data, s + 16)?;
                if rva >= start && rva - start < size {
                    let off = dword(data, s + 20)?.checked_add(rva - start)?;
                    return (off < data.len()).then_some(off);
                }
            }
            None
        };
        let export_rva = dword(data, pe + 24 + 112)?;
        let export_size = dword(data, pe + 24 + 116)?;
        let table = offset(export_rva)?;
        let names = dword(data, table + 24)?;
        if names > 65536 {
            return None;
        }
        let pointers = offset(dword(data, table + 32)?)?;
        let ordinals = offset(dword(data, table + 36)?)?;
        let functions = offset(dword(data, table + 28)?)?;
        for i in 0..names {
            let name = offset(dword(data, pointers + i * 4)?)?;
            if data.get(name..name.checked_add(wanted.len())?) == Some(wanted)
                && data.get(name + wanted.len()) == Some(&0)
            {
                let ordinal = word(data, ordinals + i * 2)? as usize;
                if ordinal >= dword(data, table + 20)? {
                    return None;
                }
                let function = dword(data, functions + ordinal * 4)?;
                return Some(
                    function != 0
                        && offset(function).is_some()
                        && !(export_rva..export_rva.checked_add(export_size)?).contains(&function),
                );
            }
        }
        Some(false)
    })()
    .unwrap_or(false)
}
