use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=../../Cargo.toml");
    let manifest = fs::read_to_string("../../Cargo.toml").expect("read launcher version");
    let version = manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = "))
        .expect("launcher package version")
        .trim_matches('"');
    println!("cargo:rustc-env=FLIGHTDECK_UI_VERSION={version}");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo provides OUT_DIR"));
    println!("cargo:rerun-if-changed=../../ui/icons.svg");
    let symbols = fs::read_to_string("../../ui/icons.svg").expect("read original icon paths");
    let mut icons = String::from("fn icon_source(name: &str) -> &'static str { match name {\n");
    for name in [
        "home",
        "settings",
        "download",
        "folder",
        "mods",
        "pulse",
        "flask",
        "monitor",
        "check-circle",
        "play",
        "drive",
        "check",
        "arrow",
    ] {
        let tag = format!("<symbol id=\"i-{name}\" viewBox=\"0 0 24 24\">");
        let body = symbols
            .split_once(&tag)
            .and_then(|(_, rest)| rest.split_once("</symbol>"))
            .map(|(body, _)| body)
            .expect("icon exists in the original UI");
        icons.push_str(&format!("{name:?} => {body:?},\n"));
    }
    icons.push_str("_ => panic!(\"unknown original icon\"), } }\n");
    fs::write(out.join("icons.rs"), icons).expect("write native icon paths");
    let source = PathBuf::from("../../ui/manrope-variable.woff2");
    println!("cargo:rerun-if-changed={}", source.display());
    let compressed = out.join("manrope-variable.woff2");
    fs::copy(source, &compressed).expect("copy the existing, OFL-licensed Manrope font");
    let result = Command::new("woff2_decompress")
        .arg(&compressed)
        .status()
        .expect("the native UI needs woff2_decompress at build time; see README.md");
    assert!(
        result.success(),
        "could not decode the existing Manrope font"
    );
}
