// SPDX-License-Identifier: MIT
//! Exercise the isolated Rust HIP ABI against a controllable native driver fixture.
#![allow(clippy::unwrap_used)]
use serde_json::Value;
use std::{fs, process::Command};

const HIP: &str = r#"
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
static int unsupported(void) { return getenv("TEST_HIP_UNSUPPORTED") != NULL; }
int hipInit(unsigned flags) { return flags ? 1 : 0; }
int hipRuntimeGetVersion(int *v) { *v = 70100000; return 0; }
int hipGetDeviceCount(int *v) { *v = getenv("TEST_HIP_BAD_COUNT") ? 17 : 1; return 0; }
int hipGetDevicePropertiesR0600(void *p, int index) {
    if (index) return 1;
    memset(p, 0, 1472);
    memcpy(p, "AMD fixture", 12);
    memcpy((char *)p + 1160, unsupported() ? "gfx1100" : getenv("TEST_HIP_RDNA2") ? "gfx1030" : "gfx1201", 8);
    return 0;
}
int hipDeviceGetPCIBusId(char *p, int len, int index) {
    if (index || len < 13) return 1;
    memcpy(p, "0000:03:00.0", 13); return 0;
}
int hipSetDevice(int index) { return index ? 1 : 0; }
int hipMalloc(void **p, size_t n) {
    /* Unsupported GPUs must never be tested by allocating inference buffers. */
    if (unsupported()) abort();
    *p = malloc(n); return *p ? 0 : 1;
}
int hipFree(void *p) { free(p); return 0; }
int hipMemcpy(void *dst, const void *src, size_t n, int kind) {
    if (getenv("TEST_HIP_COPY_FAIL")) return 1;
    if (kind != 1 && kind != 2) return 1;
    memcpy(dst, src, n); return 0;
}
"#;

#[test]
fn hip_probe_checks_memory_and_rejects_driver_failures_in_child() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("hip.c");
    let library = temp.path().join("libfixture.so");
    fs::write(&source, HIP).unwrap();
    let compiled = Command::new("cc")
        .args(["-shared", "-fPIC", "-Wall", "-Wextra", "-Werror"])
        .arg(&source)
        .arg("-o")
        .arg(&library)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let probe = |variable: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_flightdeck-rust"));
        command
            .args(["--native-probe", "hip"])
            .env("FLIGHTDECK_HIP_LIBRARY", &library);
        for name in [
            "TEST_HIP_UNSUPPORTED",
            "TEST_HIP_BAD_COUNT",
            "TEST_HIP_COPY_FAIL",
            "TEST_HIP_RDNA2",
        ] {
            command.env_remove(name);
        }
        if let Some(name) = variable {
            command.env(name, "1");
        }
        command.output().unwrap()
    };
    let good = probe(None);
    assert!(good.status.success());
    let value: Value = serde_json::from_slice(&good.stdout).unwrap();
    assert_eq!(value["devices"][0]["memory_roundtrip"], true);
    assert_eq!(value["devices"][0]["arch"], "gfx1201");
    assert_eq!(value["devices"][0]["pci_bus_id"], "0000:03:00.0");
    assert_eq!(value["neural_rendering_verified"], false);
    let rdna2 = probe(Some("TEST_HIP_RDNA2"));
    assert!(rdna2.status.success());
    let value: Value = serde_json::from_slice(&rdna2.stdout).unwrap();
    assert_eq!(value["devices"][0]["memory_roundtrip"], true);
    assert_eq!(value["devices"][0]["arch"], "gfx1030");
    assert_eq!(value["neural_rendering_verified"], false);
    let unsupported = probe(Some("TEST_HIP_UNSUPPORTED"));
    assert!(unsupported.status.success());
    let value: Value = serde_json::from_slice(&unsupported.stdout).unwrap();
    assert_eq!(value["devices"][0]["supported"], false);
    assert_eq!(value["devices"][0]["memory_roundtrip"], false);
    for failure in ["TEST_HIP_BAD_COUNT", "TEST_HIP_COPY_FAIL"] {
        let result = probe(Some(failure));
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn vulkan_pci_identity_is_optional_bounded_and_compatible_with_legacy_drivers() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("vulkan.c");
    fs::write(&source, include_str!("fixtures/vulkan-pci.c")).unwrap();
    for variant in ["full", "NO_PCI_ENUMERATOR", "NO_PROPERTIES2"] {
        let directory = temp.path().join(variant);
        fs::create_dir(&directory).unwrap();
        let mut compiler = Command::new("cc");
        compiler.args(["-shared", "-fPIC", "-Wall", "-Wextra", "-Werror"]);
        if variant != "full" {
            compiler.arg(format!("-D{variant}"));
        }
        let compiled = compiler
            .arg(&source)
            .arg("-o")
            .arg(directory.join("libvulkan.so.1"))
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        for mode in [
            "ready",
            "unadvertised",
            "enumeration-error",
            "oversized-count",
            "growing-count",
            "unfilled",
            "invalid-domain",
            "invalid-bus",
            "invalid-device",
            "invalid-function",
            "vulkan10",
        ] {
            let output = Command::new(env!("CARGO_BIN_EXE_flightdeck-rust"))
                .args(["--native-probe", "graphics"])
                .env("LD_LIBRARY_PATH", &directory)
                .env("TEST_VULKAN_MODE", mode)
                .output()
                .unwrap();
            assert!(output.status.success(), "{variant}/{mode}: {output:?}");
            let result: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(result["status"], "ready");
            let gpu = &result["devices"][0];
            assert_eq!(gpu["device_id"], 0x73bf);
            assert_eq!(gpu["vendor_id"], 0x1002);
            assert_eq!(gpu["type"], 2);
            assert_eq!(gpu["name"], "Radeon fixture with different driver name");
            if variant == "NO_PROPERTIES2" || mode == "vulkan10" {
                assert!(gpu.get("device_uuid").is_none());
            } else {
                assert_eq!(gpu["device_uuid"], "0102030405060708090a0b0c0d0e0f10");
            }
            if variant == "full" && mode == "ready" {
                assert_eq!(gpu["pci_bus_id"], "0000:03:00.0");
            } else {
                assert!(gpu.get("pci_bus_id").is_none(), "{variant}/{mode}: {gpu}");
            }
        }
    }
}
