// SPDX-License-Identifier: MIT
//! Synthetic native and source archives only; no compiler, Wine, account or network.
use super::*;
use std::io::Read;

struct Fixture {
    temporary: tempfile::TempDir,
    sources: BTreeMap<String, Vec<u8>>,
    source_manifest: Value,
    hashes: serde_json::Map<String, Value>,
    lock: Value,
    manifest: Value,
}
impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().expect("temp");
        let mut sources: BTreeMap<String, Vec<u8>> = [
            ("wine-src/LICENSE", "Synthetic Wine license marker\n"),
            ("xodus-src/LICENSE", "Synthetic Xodus license marker\n"),
            ("xodus-src/Cargo.lock", "Synthetic locked source marker\n"),
        ]
        .map(|(name, data)| (name.to_string(), data.as_bytes().to_vec()))
        .into();
        for name in HELPER_SOURCES {
            sources.insert(
                name.into(),
                format!("Synthetic helper input: {name}").into_bytes(),
            );
        }
        let hashes = NATIVE_FILES
            .iter()
            .map(|name| {
                (
                    name.to_string(),
                    digest(format!("Synthetic binary: {name}").as_bytes()).into(),
                )
            })
            .collect();
        let mut fixture = Self {
            temporary,
            source_manifest: json!({"format": 1, "files": {}}),
            sources,
            hashes,
            lock: Value::Null,
            manifest: Value::Null,
        };
        fixture.write(
            "repo/Cargo.toml",
            b"[package]\nname = \"flightdeck-linux\"\nversion = \"9.8.7\"\n",
        );
        let script =
            fs::read(crate::util::root().join("scripts/build-compat.sh")).expect("build script");
        fixture.write("repo/scripts/build-compat.sh", &script);
        fixture.write("repo/BUILDING.md", b"Synthetic build guidance\n");
        fixture.write("repo/compat/native-build.Dockerfile", b"FROM scratch\n");
        fixture.write(
            "repo/compat/LICENSES/LGPL.txt",
            b"Synthetic license preservation marker\n",
        );
        fixture.write(
            "repo/compat/LICENSES/rust/example-MIT.txt",
            b"Synthetic pinned Rust license\n",
        );
        for (name, data) in fixture.sources.clone() {
            fixture.write(&format!("stage/{name}"), &data);
            fixture.source_manifest["files"][name] = digest(&data).into();
        }
        for name in NATIVE_FILES {
            fixture.write(
                &format!("stage/artifacts/{name}"),
                format!("Synthetic binary: {name}").as_bytes(),
            );
        }
        fixture.lock = json!({"native": {"files": fixture.hashes, "features": NATIVE_FEATURES, "cli_features": CLI_FEATURES}});
        fixture.manifest = json!({"format": 1, "files": fixture.hashes, "features": NATIVE_FEATURES, "cli_features": CLI_FEATURES});
        fixture.sync();
        fixture.write(
            "vendor/example/source.rs",
            b"// Synthetic dependency source\n",
        );
        fixture.write(
            "vendor-config.toml",
            b"[source.vendored-sources]\ndirectory = \"example\"\n",
        );
        fixture.write("supplements/manifest.json", b"{\"files\":[]}\n");
        fixture
    }
    fn path(&self, name: &str) -> PathBuf {
        self.temporary.path().join(name)
    }
    fn write(&self, name: &str, data: &[u8]) {
        let path = self.path(name);
        fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        fs::write(path, data).expect("write");
    }
    /// Rewrites the three manifests so that they describe each other again.
    fn sync(&mut self) {
        let source = self.source_manifest.to_string();
        self.write("stage/source-manifest.json", source.as_bytes());
        self.manifest["files"] = Value::Object(self.hashes.clone());
        self.lock["native"]["files"] = Value::Object(self.hashes.clone());
        self.manifest["source_manifest_sha256"] = digest(source.as_bytes()).into();
        self.write(
            "stage/artifacts/manifest.json",
            self.manifest.to_string().as_bytes(),
        );
        self.write(
            "repo/compat/bootstrap.lock.json",
            self.lock.to_string().as_bytes(),
        );
    }
    fn create(&self, output: &str) -> Result<()> {
        let (stage, vendor, config, supplements, output) = (
            self.path("stage"),
            self.path("vendor"),
            self.path("vendor-config.toml"),
            self.path("supplements"),
            self.path(output),
        );
        let job = Release {
            stage: &stage,
            vendor: &vendor,
            vendor_config: &config,
            graph: Path::new("unused"),
            supplements: &supplements,
            toolchain_notices: &[],
            output: &output,
        };
        create_with(
            &self.path("repo"),
            &job,
            b"Synthetic complete notice\n".to_vec(),
            json!([]),
        )
    }
}

fn members(archive: &Path) -> BTreeMap<String, (Vec<u8>, u32)> {
    let stream = flate2::read::GzDecoder::new(fs::File::open(archive).expect("archive"));
    let mut found = BTreeMap::new();
    for entry in tar::Archive::new(stream).entries().expect("entries") {
        let mut entry = entry.expect("entry");
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("member");
        let name = entry.path().expect("path").to_string_lossy().into_owned();
        assert_eq!(
            entry.header().entry_type(),
            tar::EntryType::Regular,
            "{name}"
        );
        assert_eq!(entry.header().mtime().expect("mtime"), 0, "{name}");
        found.insert(name, (data, entry.header().mode().expect("mode")));
    }
    found
}

#[test]
fn six_binaries_and_full_corresponding_sources_are_preserved() {
    let fixture = Fixture::new();
    fixture.create("output").expect("package");
    let native = members(&fixture.path("output/flightdeck-compat-9.8.7-linux-x86_64.tar.gz"));
    let expected: BTreeSet<String> = NATIVE_FILES
        .iter()
        .chain(&["manifest.json", "THIRD-PARTY-NOTICES.txt"])
        .map(|name| name.to_string())
        .collect();
    assert_eq!(native.keys().cloned().collect::<BTreeSet<_>>(), expected);
    let (helper, mode) = &native["bin/flightdeck-connected-storage.exe"];
    assert_eq!(*mode, 0o755);
    assert_eq!(
        fixture.hashes["bin/flightdeck-connected-storage.exe"],
        digest(helper)
    );
    assert_eq!(native["runtime/xgameruntime.dll"].1, 0o644);
    let sources = members(&fixture.path("output/flightdeck-native-sources-9.8.7.tar.gz"));
    for (name, data) in &fixture.sources {
        assert_eq!(&sources[name].0, data, "{name}");
    }
    let script =
        fs::read(crate::util::root().join("scripts/build-compat.sh")).expect("build script");
    assert!(
        sources["build-compat.sh"].0 == script,
        "the build script is shipped unchanged"
    );
    let readme = String::from_utf8_lossy(&sources["README.md"].0).into_owned();
    assert!(
        readme.contains("ConnectedStorage helper")
            && readme.contains("launcher coordinates automatic")
    );
    assert!(readme.starts_with("# Flightdeck 9.8.7 corresponding native sources\n"));
    for name in [
        "runtime/tools/ConnectedStorageWrite.h",
        "vendor/example/source.rs",
        "LICENSES/LGPL.txt",
        "LICENSES/rust/example-MIT.txt",
        "source-manifest.json",
        "dependency-inventory.json",
        "BUILDING.md",
        "native-build.Dockerfile",
    ] {
        assert!(sources.contains_key(name), "{name}");
    }
    assert_eq!(
        sources[".cargo/config.toml"].0,
        b"[source.vendored-sources]\ndirectory = \"vendor\"\n"
    );
    let validation: Value = serde_json::from_slice(
        &fs::read(fixture.path("output/validation.json")).expect("validation"),
    )
    .expect("JSON");
    assert_eq!(validation["source_files"], sources.len());
    assert_eq!(
        validation["artifacts"],
        Value::Object(fixture.hashes.clone())
    );
    assert!(
        fixture.create("output").is_err(),
        "an existing output is never reused"
    );
}

#[test]
fn missing_corresponding_helper_input_is_rejected() {
    let mut fixture = Fixture::new();
    for (index, name) in HELPER_SOURCES.iter().enumerate() {
        let files = fixture.source_manifest["files"]
            .as_object_mut()
            .expect("files");
        let checksum = files.remove(*name).expect("listed");
        fixture.sync();
        let error = fixture
            .create(&format!("missing-helper-{index}"))
            .expect_err(name);
        assert!(error.0.contains("helper sources"), "{}", error.0);
        fixture.source_manifest["files"][*name] = checksum;
    }
}

#[test]
fn unpinned_helper_or_source_change_is_rejected() {
    let fixture = Fixture::new();
    fixture.write(
        "stage/artifacts/bin/flightdeck-connected-storage.exe",
        b"different bytes",
    );
    assert!(
        fixture
            .create("output")
            .expect_err("binary")
            .0
            .contains("Build input changed")
    );
    let fixture = Fixture::new();
    fixture.write("stage/xodus-src/Cargo.lock", b"edited after the build");
    assert!(
        fixture
            .create("output")
            .expect_err("source")
            .0
            .contains("Build input changed")
    );
    let fixture = Fixture::new();
    fixture.write("stage/source-manifest.json", b"{\"format\":1,\"files\":{}}");
    assert!(
        fixture
            .create("output")
            .expect_err("manifest")
            .0
            .contains("does not belong to this build")
    );
}

#[test]
fn capability_must_exist_in_build_and_lock_as_lists() {
    let mut fixture = Fixture::new();
    let mut attempt = 0;
    for target in ["lock", "build"] {
        for value in [
            json!([]),
            json!(NATIVE_FEATURES[0]),
            json!([NATIVE_FEATURES[0]]),
            json!([NATIVE_FEATURES[1]]),
            Value::Null,
            json!([NATIVE_FEATURES[0], NATIVE_FEATURES[1], 1]),
        ] {
            fixture.lock["native"]["features"] = json!(NATIVE_FEATURES);
            fixture.manifest["features"] = json!(NATIVE_FEATURES);
            if target == "lock" {
                fixture.lock["native"]["features"] = value;
            } else {
                fixture.manifest["features"] = value;
            }
            fixture.sync();
            attempt += 1;
            let error = fixture
                .create(&format!("features-{attempt}"))
                .expect_err(target);
            assert!(
                error.0.contains("ConnectedStorage capability"),
                "{}",
                error.0
            );
        }
    }
}

#[test]
fn progress_capability_is_required_in_both_build_and_lock() {
    let mut fixture = Fixture::new();
    let mut attempt = 0;
    for target in ["lock", "build"] {
        for value in [
            Value::Null,
            json!("streaming-progress-v1"),
            json!(CLI_FEATURES[..CLI_FEATURES.len() - 1]),
            json!([1]),
        ] {
            fixture.lock["native"]["cli_features"] = json!(CLI_FEATURES);
            fixture.manifest["cli_features"] = json!(CLI_FEATURES);
            if target == "lock" {
                fixture.lock["native"]["cli_features"] = value;
            } else {
                fixture.manifest["cli_features"] = value;
            }
            fixture.sync();
            attempt += 1;
            let error = fixture.create(&format!("cli-{attempt}")).expect_err(target);
            assert!(error.0.contains("CLI capability"), "{}", error.0);
        }
    }
}

#[test]
fn old_five_binary_bundle_does_not_claim_the_new_capability() {
    let mut fixture = Fixture::new();
    fixture
        .hashes
        .remove("bin/flightdeck-connected-storage.exe");
    fixture.sync();
    assert!(
        fixture
            .create("output")
            .expect_err("five binaries")
            .0
            .contains("artifacts differ")
    );
}

#[test]
fn source_trees_with_links_or_a_git_checkout_are_refused() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink("source.rs", fixture.path("vendor/example/link.rs")).expect("link");
    assert!(
        fixture
            .create("output")
            .expect_err("link")
            .0
            .contains("Unexpected source link")
    );
    let fixture = Fixture::new();
    fixture.write("stage/xodus-src/.git/HEAD", b"ref: refs/heads/main\n");
    assert!(
        fixture
            .create("output")
            .expect_err("checkout")
            .0
            .contains("mutable Git checkout")
    );
}

#[test]
fn notices_keep_every_text_in_order_and_skip_the_rust_licence_folder() {
    let fixture = Fixture::new();
    fixture.write(
        "repo/compat/THIRD_PARTY_NOTICES.md",
        b"Synthetic provenance\n",
    );
    fixture.write(
        "repo/compat/runtime/include/vendor/nlohmann_json.hpp",
        b"/*\nSynthetic JSON licence\n*/\nnamespace nlohmann {}\n",
    );
    for name in [
        "libs/xml2/COPYING",
        "libs/c++/LICENSE.TXT",
        "libs/c++abi/LICENSE.TXT",
        "libs/compiler-rt/LICENSE.TXT",
    ] {
        fixture.write(
            &format!("stage/wine-src/{name}"),
            format!("Synthetic {name}\n").as_bytes(),
        );
    }
    fixture.write("crate/LICENSE-MIT", b"Synthetic crate licence\n");
    fixture.write("crate/src/lib.rs", b"// no licence here\n");
    fixture.write("crate/Cargo.toml", b"[package]\n");
    fixture.write(
        "stage/xodus-src/crates/imported/LICENSE",
        b"Synthetic imported MIT license\n",
    );
    fixture.write(
        "toolchain/libgcc-notice.txt",
        b"Synthetic toolchain notice\n",
    );
    let extra = b"Synthetic supplementary licence\n";
    fixture.write("supplements/extra/LICENSE", extra);
    fixture.write(
        "supplements/manifest.json",
        json!({"files": [{"package": "local", "version": "1.0.0", "path": "extra/LICENSE", "sha256": digest(extra)}]})
            .to_string()
            .as_bytes(),
    );
    let graph = json!([
        {"name": "example", "version": "2.0.0", "source": "registry+https://example.invalid",
            "manifest_path": fixture.path("crate/Cargo.toml"), "license": "MIT", "repository": "https://example.invalid/example"},
        {"name": "local", "version": "1.0.0", "source": null, "manifest_path": fixture.path("stage/xodus-src/Cargo.toml"),
            "license": null, "repository": null},
        {"name": "imported", "version": "1.0.0", "source": null,
            "manifest_path": fixture.path("stage/xodus-src/crates/imported/Cargo.toml"),
            "license": "MIT", "repository": "https://example.invalid/imported"}
    ]);
    fixture.write("graph.json", graph.to_string().as_bytes());
    let (stage, vendor, config, supplements, graph, output) = (
        fixture.path("stage"),
        fixture.path("vendor"),
        fixture.path("vendor-config.toml"),
        fixture.path("supplements"),
        fixture.path("graph.json"),
        fixture.path("output"),
    );
    let toolchain = [fixture.path("toolchain/libgcc-notice.txt")];
    let job = Release {
        stage: &stage,
        vendor: &vendor,
        vendor_config: &config,
        graph: &graph,
        supplements: &supplements,
        toolchain_notices: &toolchain,
        output: &output,
    };
    let (notice, inventory) = notices(&fixture.path("repo"), &job).expect("notices");
    let notice = String::from_utf8(notice).expect("text");
    let order = [
        "Flightdeck 9.8.7 native compatibility components",
        "Flightdeck compatibility / LGPL.txt",
        "Component provenance",
        "nlohmann JSON 3.6.1\n/*\nSynthetic JSON licence\n*/",
        "Wine and bundled library / LICENSE",
        "Wine and bundled library / libs/compiler-rt/LICENSE.TXT",
        "Build runtime / libgcc-notice.txt",
        "example 2.0.0 / LICENSE-MIT",
        "local 1.0.0 / LICENSE\n",
        "Synthetic supplementary licence",
        "imported 1.0.0 / LICENSE\n",
        "Synthetic imported MIT license",
    ];
    let positions: Vec<usize> = order
        .iter()
        .map(|part| {
            notice
                .find(part)
                .unwrap_or_else(|| panic!("missing {part}"))
        })
        .collect();
    assert!(positions.is_sorted(), "{positions:?}");
    assert!(
        !notice.contains("Synthetic pinned Rust license") && !notice.contains("no licence here")
    );
    assert_eq!(inventory[0]["license"], "MIT");
    assert_eq!(inventory[0]["notices"][0]["name"], "LICENSE-MIT");
    assert_eq!(inventory[1]["license"], "GPL-3.0-only");
    assert_eq!(inventory[1]["notices"].as_array().map(Vec::len), Some(2));
    assert_eq!(inventory[2]["license"], "MIT");
    assert_eq!(
        inventory[2]["notices"][0]["sha256"],
        digest(b"Synthetic imported MIT license\n")
    );
    // A package without any licence text stops the packaging.
    fs::remove_file(fixture.path("crate/LICENSE-MIT")).expect("remove");
    assert!(
        notices(&fixture.path("repo"), &job)
            .expect_err("no text")
            .0
            .contains("Missing original license text")
    );
}

#[test]
fn staged_archives_cannot_write_outside_their_destination() {
    use crate::components::fixtures::raw_member;
    let pack = |members: &[Vec<u8>]| {
        let mut data: Vec<u8> = members.concat();
        data.extend_from_slice(&[0; 1024]);
        data
    };
    let temp = tempfile::tempdir().expect("temp");
    let good = pack(&[
        raw_member(
            "pax_global_header",
            b'g',
            b"52 comment=0000000000000000000000000000000000000000\n",
        ),
        raw_member("root/dir/file.txt", b'0', b"content"),
        raw_member("root/run.sh", b'0', b"#!/bin/sh\n"),
    ]);
    unpack(&good[..], &temp.path().join("good"), true).expect("unpack");
    assert_eq!(
        fs::read(temp.path().join("good/dir/file.txt")).expect("file"),
        b"content"
    );
    let two_roots = pack(&[
        raw_member("a/file", b'0', b""),
        raw_member("b/file", b'0', b""),
    ]);
    assert!(unpack(&two_roots[..], &temp.path().join("roots"), true).is_err());
    for bad in [
        raw_member("root/../escape", b'0', b""),
        raw_member("/absolute", b'0', b""),
        raw_member("root/device", b'3', b""),
        raw_member("root/fifo", b'6', b""),
        raw_member("root/hard", b'1', b""),
    ] {
        let destination = temp.path().join("bad");
        let _ = fs::remove_dir_all(&destination);
        assert!(unpack(&pack(&[bad])[..], &destination, false).is_err());
        assert!(!temp.path().join("escape").exists());
    }
}
