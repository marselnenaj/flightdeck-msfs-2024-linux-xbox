// SPDX-License-Identifier: MIT
//! Synthetic GStreamer ABI: a cached factory can exist while construction fails.
#![allow(clippy::unwrap_used)]
use serde_json::Value;
use std::{fs, process::Command};

#[test]
fn media_probe_constructs_elements_instead_of_trusting_cached_factories() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("gstreamer.c");
    fs::write(
        &source,
        r#"
#include <stdlib.h>
#include <string.h>
static int element;
static int fork_disabled, segtrap_disabled;
void gst_registry_fork_set_enabled(int enabled) { fork_disabled = !enabled; }
void gst_segtrap_set_enabled(int enabled) { segtrap_disabled = !enabled; }
int gst_init_check(int *argc, char ***argv, void **error) { return fork_disabled && segtrap_disabled; }
void *gst_element_factory_find(const char *name) { return &element; }
void *gst_element_factory_make(const char *name, const char *instance) {
    const char *missing = getenv("TEST_MISSING_ELEMENT");
    return missing && !strcmp(missing, name) ? 0 : &element;
}
void g_object_unref(void *object) {}
"#,
    )
    .unwrap();
    assert!(
        Command::new("cc")
            .args(["-shared", "-fPIC", "-o"])
            .arg(temp.path().join("libgstreamer-1.0.so.0"))
            .arg(&source)
            .status()
            .unwrap()
            .success()
    );
    for missing in [
        "",
        "avdec_h264",
        "h264parse",
        "qtdemux",
        "videoconvert",
        "decodebin",
        "capsfilter",
        "deinterlace",
        "videoflip",
        "capssetter",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_flightdeck-rust"))
            .args(["--native-probe", "media"])
            .env_clear()
            .env("LD_LIBRARY_PATH", temp.path())
            .env("TEST_MISSING_ELEMENT", missing)
            .output()
            .unwrap();
        assert!(output.status.success());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            value["state"],
            if missing.is_empty() {
                "ready"
            } else {
                "incomplete"
            }
        );
        assert_eq!(value["h264"], missing != "avdec_h264");
        for name in flightdeck::native_probe::MEDIA_ELEMENTS {
            assert_eq!(value["elements"][name], name != missing);
        }
        assert!(!String::from_utf8_lossy(&output.stdout).contains(temp.path().to_str().unwrap()));
    }
}
