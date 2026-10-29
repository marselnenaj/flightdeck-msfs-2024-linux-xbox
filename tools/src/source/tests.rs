// SPDX-License-Identifier: MIT
use super::*;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use std::io::Read;

struct Tree(tempfile::TempDir);
impl Tree {
    fn new() -> Self {
        let tree = Self(tempfile::tempdir().expect("temp"));
        tree.put("scripts/example.py", "print('example')\n");
        tree
    }
    fn root(&self) -> &Path {
        self.0.path()
    }
    fn put(&self, name: &str, content: impl AsRef<[u8]>) {
        let path = self.root().join(name);
        fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        fs::write(path, content).expect("write");
    }
    fn remove(&self, name: &str) {
        fs::remove_file(self.root().join(name)).expect("remove");
    }
    fn names(&self) -> Vec<String> {
        inspect(self.root())
            .expect("inspection")
            .into_keys()
            .collect()
    }
    fn error(&self) -> String {
        inspect(self.root()).expect_err("rejected").0
    }
}

#[test]
fn only_explicit_source_paths_are_exported() {
    let tree = Tree::new();
    for directory in ["private", "build", "games", "runtime"] {
        tree.put(&format!("{directory}/secret.txt"), "not for release");
    }
    assert_eq!(tree.names(), ["scripts/example.py"]);
}

#[test]
fn reviewed_mobiflight_builder_source_and_project_are_exported() {
    let tree = Tree::new();
    tree.put(
        "compat/mobiflight/Program.cs",
        "// reviewed patch builder\n",
    );
    tree.put(
        "compat/mobiflight/HidPatch.csproj",
        "<Project Sdk=\"Microsoft.NET.Sdk\" />\n",
    );
    assert!(
        tree.names()
            .contains(&"compat/mobiflight/Program.cs".to_string())
    );
    assert!(
        tree.names()
            .contains(&"compat/mobiflight/HidPatch.csproj".to_string())
    );
}

#[test]
fn retired_application_and_installers_cannot_return() {
    let tree = Tree::new();
    for name in RETIRED {
        tree.put(name, "obsolete application or build entry point");
        assert!(tree.error().contains("Retired Python"), "{name}");
        tree.remove(name);
    }
}

#[test]
fn binary_extension_and_magic_are_rejected() {
    let tree = Tree::new();
    tree.put("scripts/bad.dll", "dummy");
    assert!(tree.error().contains("Unrecognized"));
    tree.remove("scripts/bad.dll");
    tree.put("scripts/bad.py", b"MZfake");
    assert!(tree.error().contains("Binary"));
    tree.put("scripts/bad.py", b"text with a \0 byte");
    assert!(tree.error().contains("Binary"));
}

#[test]
fn rust_sources_and_lockfile_are_exported_without_build_outputs() {
    let tree = Tree::new();
    for name in [
        "Cargo.toml",
        "Cargo.lock",
        "native/main.rs",
        "tests/native_files.rs",
        "tools/src/main.rs",
    ] {
        tree.put(name, "// synthetic source\n");
    }
    for name in [
        "target/debug/flightdeck-rust",
        "native/target/debug/fixture",
    ] {
        tree.put(name, b"\x7fELFsynthetic binary");
    }
    assert_eq!(
        tree.names(),
        [
            "Cargo.lock",
            "Cargo.toml",
            "native/main.rs",
            "scripts/example.py",
            "tests/native_files.rs",
            "tools/src/main.rs"
        ]
    );
}

#[test]
fn only_exact_reviewed_artwork_is_allowed() {
    let tree = Tree::new();
    let original = fs::read(crate::util::root().join("ui/flight-panorama.png")).expect("artwork");
    tree.put("ui/flight-panorama.png", original);
    assert!(tree.names().contains(&"ui/flight-panorama.png".to_string()));
    tree.put("ui/flight-panorama.png", "replacement private image");
    assert!(tree.error().contains("Unreviewed artwork"));
    tree.remove("ui/flight-panorama.png");
    tree.put("ui/private-screenshot.png", "private");
    assert!(tree.error().contains("Unrecognized"));
}

#[test]
fn neural_sources_and_license_export_without_native_binaries() {
    let tree = Tree::new();
    tree.put(
        "compat/neural-rdna2/test_wave.hip",
        "// synthetic HIP source\n",
    );
    tree.put("compat/neural-rdna2/UPSTREAM-LICENSE", "MIT License\n");
    let names = tree.names();
    assert!(names.contains(&"compat/neural-rdna2/test_wave.hip".to_string()));
    assert!(names.contains(&"compat/neural-rdna2/UPSTREAM-LICENSE".to_string()));
    tree.put(
        "compat/neural-rdna2/test_wave.hip",
        b"\x7fELFsynthetic binary",
    );
    assert!(tree.error().contains("Binary"));
}

#[test]
fn symlink_and_personal_path_are_rejected() {
    let tree = Tree::new();
    let link = tree.root().join("scripts/link.py");
    std::os::unix::fs::symlink(tree.root().join("scripts/example.py"), &link).expect("link");
    assert!(tree.error().contains("symlinks"));
    fs::remove_file(link).expect("remove");
    std::os::unix::fs::symlink(tree.root().join("scripts"), tree.root().join("docs"))
        .expect("directory link");
    assert!(tree.error().contains("symlinks"));
    fs::remove_file(tree.root().join("docs")).expect("remove");
    // Assembled, so that this regression source stays free of machine paths.
    tree.put(
        "scripts/bad.py",
        ["/", "home/", "synthetic-user/", "secret"].concat(),
    );
    assert!(tree.error().contains("Personal absolute"));
}

#[test]
fn all_top_level_symlinks_are_rejected_including_dangling() {
    let tree = Tree::new();
    for name in TOP_LEVEL {
        for target in ["scripts/example.py", "missing"] {
            let link = tree.root().join(name);
            std::os::unix::fs::symlink(tree.root().join(target), &link).expect("link");
            assert!(tree.error().contains("symlinks"), "{name} -> {target}");
            fs::remove_file(link).expect("remove");
        }
    }
}

#[test]
fn credential_account_and_browser_files_are_rejected() {
    let tree = Tree::new();
    for name in [
        ".env",
        ".env.production",
        "tokens.json",
        "account.json",
        "accounts.json",
        "credentials.yaml",
        "xodus-keyring.ron",
        "msal_token_cache.json",
        ".netrc",
        "cookies.json",
        "storage-state.json",
        "Local Storage/entry.json",
        "private/run.json",
    ] {
        let name = format!("scripts/{name}");
        tree.put(&name, "{}");
        assert!(tree.error().contains("Private data file"), "{name}");
        tree.remove(&name);
    }
}

#[test]
fn templates_auth_code_project_links_and_licenses_are_allowed() {
    let tree = Tree::new();
    for name in [".env.example", ".env.sample", ".env.template"] {
        tree.put(
            &format!("scripts/{name}"),
            "ACCESS_TOKEN=replace-me\nREGION=US\n",
        );
    }
    let safe = "access_token = get_token()\nrefresh_token = \"synthetic-placeholder\"\nauthorization = \"Bearer ${TOKEN}\"\nexample = \"eyJTest.Exact_Payload.Signature-123\"\nhttps://github.com/example-org/example-project\nCopyright Example Contributors <maintainer@example.invalid>\n";
    tree.put("scripts/store_account.py", safe);
    tree.put("scripts/token-parser.py", safe);
    assert_eq!(tree.names().len(), 6);
    // A template name cannot bypass the content scan.
    tree.put(
        "scripts/.env.example",
        "access_token=\"A9b8C7d6E5f4G3h2I1j0K9l8M7n6O5p4\"",
    );
    assert!(tree.error().contains("credential literal"));
}

#[test]
fn private_key_variants_embedded_and_patch_form_are_rejected() {
    let tree = Tree::new();
    let body = STANDARD.encode((0..96).collect::<Vec<u8>>());
    for label in [
        "PRIVATE KEY",
        "RSA PRIVATE KEY",
        "EC PRIVATE KEY",
        "DSA PRIVATE KEY",
        "OPENSSH PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
        "PGP PRIVATE KEY BLOCK",
    ] {
        let block = format!(
            "-----{}{label}-----\n{body}\n-----END {label}-----",
            "BEGIN "
        );
        for value in [
            block.clone(),
            json!({"key": block}).to_string(),
            format!("+{}", block.replace('\n', "\n+")),
        ] {
            tree.put("scripts/fixture.txt", value);
            let error = tree.error();
            assert!(error.contains("Private key material"), "{label}");
            assert!(!error.contains(&body));
        }
    }
    // A header mention or a deliberately redacted documentation block is harmless.
    tree.put(
        "scripts/fixture.txt",
        format!(
            "-----{}PRIVATE KEY-----\n<redacted>\n-----END PRIVATE KEY-----",
            "BEGIN "
        ),
    );
    inspect(tree.root()).expect("redacted block");
}

#[test]
fn structured_signed_token_is_rejected_without_claim_output() {
    let tree = Tree::new();
    let segment = |value: Value| URL_SAFE_NO_PAD.encode(value.to_string());
    let signature = URL_SAFE_NO_PAD.encode((0..64).collect::<Vec<u8>>());
    let header = segment(json!({"alg": "RS256", "typ": "JWT"}));
    let payload = segment(json!({"sub": "synthetic-subject", "exp": 1}));
    let token = [header.as_str(), &payload, &signature].join(".");
    for value in [
        token.clone(),
        format!("prefix-segment.{token}"),
        format!("Authorization: {token}."),
    ] {
        tree.put("scripts/fixture.txt", value);
        let error = tree.error();
        assert!(error.contains("signed-token data"));
        assert!(!error.contains(&token) && !error.contains("synthetic-subject"));
    }
    for value in [
        "eyJTest.Exact_Payload.Signature-123".to_string(),
        [
            segment(json!({"alg": "none"})).as_str(),
            &payload,
            &signature,
        ]
        .join("."),
        [
            segment(json!({"alg": ["RS256"]})).as_str(),
            &payload,
            &signature,
        ]
        .join("."),
        [
            header.as_str(),
            &segment(json!("not an object")),
            &signature,
        ]
        .join("."),
        [header.as_str(), "not-valid-json", &signature].join("."),
        format!("x{token}x-"),
        format!("{header}..{payload}.{signature}"),
    ] {
        tree.put("scripts/fixture.txt", value);
        inspect(tree.root()).expect("not a signed token");
    }
}

#[test]
fn private_jwk_is_rejected_public_and_placeholder_keys_are_allowed() {
    let tree = Tree::new();
    let material = URL_SAFE_NO_PAD.encode((0..32).collect::<Vec<u8>>());
    for (kind, field) in [("RSA", "d"), ("EC", "d"), ("OKP", "d"), ("oct", "k")] {
        tree.put(
            "scripts/fixture.json",
            json!({"keys": [{"kty": kind, field: material}]}).to_string(),
        );
        assert!(tree.error().contains("Private JWK material"), "{kind}");
    }
    for item in [
        json!({"kty": "RSA", "n": material, "e": "AQAB"}),
        json!({"kty": "EC", "x": material, "y": material}),
        json!({"kty": "RSA", "d": "<private component>"}),
        json!({"kty": ["RSA", "EC"], "d": "schema field"}),
        json!({"schema": {"kty": "string", "d": "string"}}),
    ] {
        tree.put("scripts/fixture.json", item.to_string());
        inspect(tree.root()).expect("public or placeholder key");
    }
}

#[test]
fn access_keys_and_credential_literals_are_rejected_without_values() {
    let tree = Tree::new();
    let material = "A9b8C7d6E5f4G3h2I1j0K9l8M7n6O5p4Q3r2S1t0U9v8W7x6Y5z4";
    for value in [
        format!("ghp_{material}"),
        format!("github_pat_{material}{material}"),
        format!("sk-proj-{material}"),
        format!("refresh_token=\"{material}\""),
        format!("{{\"client_secret\":\"{material}\"}}"),
        format!("Bearer {material}"),
        format!("XBL3.0 x=1234567890;{material}"),
    ] {
        tree.put("scripts/fixture.txt", value);
        let error = tree.error();
        assert!(error.contains("Embedded"), "{error}");
        assert!(!error.contains(material));
    }
    for value in [
        format!("access_token = \"{}\"", "A".repeat(80)),
        format!("Bearer {}", "0".repeat(80)),
        "client_secret=\"${CLIENT_SECRET}\"".to_string(),
        "XBL3.0 x={hash};{token}".to_string(),
        // A token glued into a longer identifier has no word boundary.
        format!("ghp_{}", material.repeat(6)),
    ] {
        tree.put("scripts/fixture.txt", value);
        inspect(tree.root()).expect("placeholder");
    }
}

fn members(archive: &Path) -> BTreeMap<String, (Vec<u8>, u32, u64)> {
    let mut found = BTreeMap::new();
    let stream = flate2::read::GzDecoder::new(fs::File::open(archive).expect("archive"));
    for entry in tar::Archive::new(stream).entries().expect("entries") {
        let mut entry = entry.expect("entry");
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("member");
        let header = entry.header();
        let name = entry.path().expect("path").to_string_lossy().into_owned();
        let attributes = (
            data,
            header.mode().expect("mode"),
            header.mtime().expect("mtime"),
        );
        assert!(found.insert(name, attributes).is_none());
    }
    found
}

#[test]
fn changed_or_missing_source_is_rejected_before_output_creation() {
    let tree = Tree::new();
    let output = tree.root().join("build/source.tar.gz");
    let files = inspect(tree.root()).expect("inspection");
    tree.put("scripts/example.py", "print('changed after inspection')\n");
    let error = archive(tree.root(), &output, &files, 0).expect_err("changed");
    assert!(error.0.contains("Source changed after inspection"));
    assert!(!output.parent().expect("parent").exists());
    tree.remove("scripts/example.py");
    assert!(archive(tree.root(), &output, &files, 0).is_err());
    assert!(!output.parent().expect("parent").exists());
}

#[test]
fn archive_matches_its_manifest_and_marks_scripts_executable() {
    let tree = Tree::new();
    tree.put("install.sh", "#!/bin/sh\nexit 0\n");
    tree.put("Install Flightdeck.desktop", "[Desktop Entry]\n");
    let long = format!("docs/{}/note.md", "long-directory-name".repeat(6));
    tree.put(&long, "A path longer than one tar header field.\n");
    let output = tree.root().join("build/source.tar.gz");
    let files = inspect(tree.root()).expect("inspection");
    archive(tree.root(), &output, &files, 1_700_000_000).expect("archive");
    let members = members(&output);
    let manifest: Value =
        serde_json::from_slice(&members["flightdeck-linux/SOURCE-MANIFEST.json"].0)
            .expect("manifest");
    assert_eq!(manifest["format"], 1);
    let listed = manifest["files"].as_object().expect("files");
    let mut expected: BTreeSet<String> = listed
        .keys()
        .map(|name| format!("flightdeck-linux/{name}"))
        .collect();
    expected.insert("flightdeck-linux/SOURCE-MANIFEST.json".into());
    assert_eq!(members.keys().cloned().collect::<BTreeSet<_>>(), expected);
    assert!(listed.contains_key(&long));
    for (name, hash) in listed {
        let (data, mode, mtime) = &members[&format!("flightdeck-linux/{name}")];
        assert_eq!(&digest(data), hash.as_str().expect("hash"));
        assert_eq!(*mtime, 1_700_000_000);
        let executable = ["install.sh", "Install Flightdeck.desktop"].contains(&name.as_str());
        assert_eq!(*mode, if executable { 0o755 } else { 0o644 }, "{name}");
    }
}

#[test]
fn existing_archive_and_outside_destinations_remain_untouched() {
    let tree = Tree::new();
    let files = inspect(tree.root()).expect("inspection");
    tree.put("build/source.tar.gz", "existing source archive");
    let output = tree.root().join("build/source.tar.gz");
    let error = archive(tree.root(), &output, &files, 0).expect_err("exists");
    assert!(error.0.contains("Output exists"));
    assert_eq!(fs::read(&output).expect("read"), b"existing source archive");
    for outside in [
        "source.tar.gz",
        "build/../source.tar.gz",
        "docs/source.tar.gz",
    ] {
        let error = archive(tree.root(), &tree.root().join(outside), &files, 0)
            .expect_err("outside the build directory");
        assert!(
            error.0.contains("under the ignored build directory"),
            "{outside}"
        );
        assert!(!tree.root().join("source.tar.gz").exists());
    }
}

#[test]
fn corresponding_source_archives_are_recorded_as_exact_manifest_members() {
    let tree = Tree::new();
    let output = tree.root().join("build/with-media.tar.gz");
    let extra = BTreeMap::from([
        (
            "media-sources/wine-source.tar.gz".into(),
            b"pinned wine".to_vec(),
        ),
        (
            "media-sources/xgameruntime-source.tar.gz".into(),
            b"pinned gdk".to_vec(),
        ),
    ]);
    archive_extra(
        tree.root(),
        &output,
        &inspect(tree.root()).expect("inspect"),
        0,
        extra.clone(),
    )
    .expect("archive");
    let entries = members(&output);
    let manifest: Value =
        serde_json::from_slice(&entries["flightdeck-linux/SOURCE-MANIFEST.json"].0)
            .expect("manifest");
    for (name, bytes) in extra {
        assert_eq!(entries[&format!("flightdeck-linux/{name}")].0, bytes);
        assert_eq!(manifest["files"][name], digest(&bytes));
    }
}

#[test]
fn the_checkout_itself_passes_the_inspection() {
    let files = inspect(&crate::util::root()).expect("current sources");
    assert!(files.contains_key("tools/src/source.rs"));
    assert!(files.contains_key("Cargo.lock"));
}
