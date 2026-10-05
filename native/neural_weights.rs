// SPDX-License-Identifier: MIT
//! Explicit local import of pinned NR data. Never executes or downloads a DLL.
use crate::{Error, Result, error::require, files, neural_assets, neural_decode};
use clap::Args;
use rustix::fs::{self as rfs, AtFlags, Mode, RenameFlags};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Path, PathBuf},
};

pub const SCHEMA: &str = "flightdeck-nr-weights-ht-836f445d-v1";
pub const ARCHIVE_SHA: &str = "836f445d06ecd2e59bb9f17b84b91c143396fd76ccda1c9dc7fe81d5edd548f4";
pub const DLL_SHA: &str = "e16bcf15e16e13f527491cdf7845b2fe6521a738d8f7c9c721866a8496e1fc8e";
pub const MAPPING_COMMIT: &str = "15799b1600d57b849597a44be53ac892b7a2faea";
const ARCHIVE_OFFSET: usize = 0x114a160;
const ARCHIVE_BYTES: usize = 147695410;

#[derive(Args)]
pub struct Options {
    /// Your local NR DLL 310.8.0.0 or its exact WEIGHTS_HT resource archive.
    #[arg(long)]
    source: PathBuf,
    /// A new cache directory; an existing directory is never overwritten.
    #[arg(long)]
    output: PathBuf,
}

fn source_archive(data: &[u8]) -> Result<(&[u8], &'static str, String)> {
    let digest = files::sha256(data);
    let (archive, kind) = if data.len() == ARCHIVE_BYTES && digest == ARCHIVE_SHA {
        (data, "weights-ht")
    } else {
        require(
            digest == DLL_SHA,
            "Unknown NR model source. Select the pinned NR DLL 310.8.0.0 or its SHA-verified WEIGHTS_HT archive.",
        )?;
        let archive = data
            .get(ARCHIVE_OFFSET..ARCHIVE_OFFSET + ARCHIVE_BYTES)
            .ok_or(Error::Invalid("The NR model resource is truncated."))?;
        (archive, "nr-dll")
    };
    require(
        files::sha256(archive) == ARCHIVE_SHA,
        "The NR weight resource does not match the pinned model.",
    )?;
    Ok((archive, kind, digest))
}

pub fn source_manifest(kind: &str, digest: &str, tables: Value, scalars: Value) -> Value {
    json!({
        "schema":SCHEMA,
        "source":{"kind":kind,"sha256":digest},
        "archive_sha256":ARCHIVE_SHA,"archive_records":153,
        "upstream_commit":MAPPING_COMMIT,"layout_mode":"amd-consumer-derived",
        "runtime_ready":true,"experimental_runtime_ready":true,"nvidia_equivalence":false,
        "converter":"flightdeck-rust-v1","scalar_records":scalars,"tables":tables,
        "provenance":{
            "classification":"EXPERIMENTAL reconstructed AMD-consumer layout",
            "coefficients":"Decoded from the exact SHA-verified WEIGHTS_HT resource; no fitted coefficients or default scales.",
            "structural_zeros":"Published sparse W2 absent connections and unused ordinary C32 mix prefix only.",
            "mapping_source":"guentra/dlss5-amd-hip-linux@3c7740e62610d4bca3fd82669002288b4293a8b4/linux/dlssnr/convert_dll.py",
            "nvidia_equivalence":false,"upstream_captured":false,
            "limitations":["C32, attention skip and 640-token bridge maps are AMD-consumer reconstructions.",
                "A complete coefficient cache does not establish image quality or successful MSFS interception."]
        }
    })
}

pub fn validate_source(value: &Value) -> bool {
    let source = &value["source"];
    value["schema"] == SCHEMA
        && value["archive_sha256"] == ARCHIVE_SHA
        && value["archive_records"] == 153
        && value["converter"] == "flightdeck-rust-v1"
        && ((source["kind"] == "weights-ht" && source["sha256"] == ARCHIVE_SHA)
            || (source["kind"] == "nr-dll" && source["sha256"] == DLL_SHA))
}

struct Cache<'a> {
    directory: &'a File,
    sizes: BTreeMap<String, usize>,
    tables: BTreeMap<String, Value>,
}
impl Cache<'_> {
    fn bytes(&mut self, name: String, bytes: Vec<u8>) -> Result<()> {
        let count = *self
            .sizes
            .get(&name)
            .ok_or(Error::Invalid("Unexpected NR coefficient table."))?;
        require(
            bytes.len() == count * 4 && !self.tables.contains_key(&name),
            "Invalid NR coefficient table size.",
        )?;
        let digest = files::sha256(&bytes);
        files::atomic_at(self.directory, &name, &bytes)?;
        self.tables
            .insert(name, json!({"count":count,"sha256":digest}));
        Ok(())
    }
    fn floats(&mut self, name: String, values: Vec<f32>) -> Result<()> {
        require(
            values.iter().all(|v| v.is_finite()),
            "The decoded NR model contains non-finite coefficients.",
        )?;
        self.bytes(
            name,
            values.into_iter().flat_map(f32::to_le_bytes).collect(),
        )
    }
}

fn write_cache(directory: &File, archive: &[u8], kind: &str, digest: &str) -> Result<Value> {
    let records = neural_decode::records(archive)?;
    let mut scalars = BTreeMap::new();
    for (&name, raw) in &records {
        if let [a, b] = **raw {
            let value = neural_decode::half(u16::from_le_bytes([a, b]));
            require(
                value.is_finite(),
                "The NR model contains a non-finite scalar.",
            )?;
            scalars.insert(name, value);
        }
    }
    let mut cache = Cache {
        directory,
        sizes: neural_assets::table_sizes(),
        tables: BTreeMap::new(),
    };
    neural_decode::decode(&records, |name, values| cache.floats(name, values))?;
    cache.bytes("hwc-to-vit.i32".into(), neural_decode::bridge(false))?;
    cache.bytes("vit-to-hwc.i32".into(), neural_decode::bridge(true))?;
    require(
        cache.tables.len() == cache.sizes.len(),
        "The decoded NR model is incomplete.",
    )?;
    let manifest = source_manifest(
        kind,
        digest,
        serde_json::to_value(cache.tables)?,
        serde_json::to_value(scalars)?,
    );
    neural_assets::validate_weight_manifest(&manifest)?;
    files::atomic_at(
        directory,
        "manifest.json",
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    // Published only as a complete new directory, never repaired in place.
    files::atomic_at(
        directory,
        "full-network.ok",
        format!("{SCHEMA}\n").as_bytes(),
    )?;
    Ok(manifest)
}

pub fn import(source: &Path, output: &Path) -> Result<Value> {
    require(
        !files::exists(output),
        "The weight-cache destination already exists. Choose a new directory.",
    )?;
    let data = files::read_public(source, 256 * 1024 * 1024)?;
    let (archive, kind, digest) = source_archive(&data)?;
    let output = std::path::absolute(output)?;
    let parent = files::directory(
        output
            .parent()
            .ok_or(Error::Invalid("Invalid cache destination."))?,
        false,
    )?;
    let name = output
        .file_name()
        .ok_or(Error::Invalid("Invalid cache destination."))?;
    let temporary = format!(".neural-weights-{}", uuid::Uuid::new_v4().simple());
    rfs::mkdirat(&parent, &temporary, Mode::from_raw_mode(0o700))?;
    let directory = files::open_at(&parent, &temporary, true, true)?;
    let mut published = false;
    let result = (|| {
        let manifest = write_cache(&directory, archive, kind, &digest)?;
        directory.sync_all()?;
        rfs::renameat_with(&parent, &temporary, &parent, name, RenameFlags::NOREPLACE)?;
        published = true;
        parent.sync_all()?;
        Ok(
            json!({"state":"weights_imported","schema":SCHEMA,"archive_sha256":ARCHIVE_SHA,
            "tables":manifest["tables"].as_object().map(|v|v.len()),"output":output,
            "experimental":true,"msfs_rendering_verified":false}),
        )
    })();
    if !published {
        for name in neural_assets::table_sizes()
            .keys()
            .map(String::as_str)
            .chain(["manifest.json", "full-network.ok"])
        {
            let _ = rfs::unlinkat(&directory, name, AtFlags::empty());
        }
        let _ = rfs::unlinkat(&parent, &temporary, AtFlags::REMOVEDIR);
    }
    result
}

pub fn cli(options: Options) -> Result<Value> {
    import(&options.source, &options.output)
}

#[cfg(test)]
#[path = "neural_weights_tests.rs"]
mod tests;
