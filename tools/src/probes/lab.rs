// SPDX-License-Identifier: MIT
//! The transport to `examples/runtime-lab.rs`: one JSON request on its
//! standard input, one JSON answer on its standard output. Prefix, graphics,
//! VR, Proton and Fenix behaviour stays in that driver and therefore in the
//! launcher's own code; nothing of it is repeated here. Build the driver
//! first: `cargo build --locked --bin flightdeck-rust --example runtime-lab`.
use super::host::{self, Environment};
use crate::util::{Failure, Result, fail, require};
use serde_json::{Map, Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const BUILD_FIRST: &str =
    "Build the Rust probes first: cargo build --locked --bin flightdeck-rust --example runtime-lab";
/// What the driver expects in `private/synthetic-probe` before it prepares
/// anything: proof that the runtime was made for a probe.
pub const MARKER: &str = "Flightdeck isolated test\n";
const SECONDS: u64 = 900;

pub struct Lab {
    binary: PathBuf,
}

/// The one discrete GPU a graphics probe runs on.
#[derive(Debug, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub uuid: String,
    pub vendor: u64,
}
pub const NVIDIA: u64 = 0x10de;

/// The request as the driver reads it; an input cannot replace the operation.
pub fn encode(operation: &str, inputs: Value) -> Result<Vec<u8>> {
    let mut request = Map::new();
    match inputs {
        Value::Null => {}
        Value::Object(inputs) => request.extend(inputs),
        _ => return fail("Probe inputs must be a JSON object"),
    }
    request.insert("operation".into(), operation.into());
    Ok(Value::Object(request).to_string().into_bytes())
}

impl Lab {
    /// `FLIGHTDECK_PROBE_BINARY` selects another built copy of the driver.
    pub fn new(root: &Path) -> Self {
        Self::at(
            std::env::var_os("FLIGHTDECK_PROBE_BINARY")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("target/debug/examples/runtime-lab")),
        )
    }
    pub fn at(binary: PathBuf) -> Self {
        Self { binary }
    }

    /// Runs one operation. A failing driver reports its own message.
    pub fn call(&self, operation: &str, inputs: Value) -> Result<Value> {
        self.call_recorded(operation, inputs, None)
    }

    pub fn call_with_stderr(&self, operation: &str, inputs: Value, log: &Path) -> Result<Value> {
        self.call_recorded(operation, inputs, Some(log))
    }

    fn call_recorded(&self, operation: &str, inputs: Value, log: Option<&Path>) -> Result<Value> {
        require(self.binary.is_file(), BUILD_FIRST)?;
        let request = encode(operation, inputs)?;
        let mut command = Command::new(host::canonical(&self.binary)?);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let finished = host::run(&mut command, Some(&request), SECONDS)?;
        if let Some(log) = log {
            host::write(log, &finished.stderr)?;
        }
        if finished.code != 0 {
            let message = String::from_utf8_lossy(&finished.stderr).trim().to_string();
            return fail(if message.is_empty() {
                "Rust probe failed".into()
            } else {
                message
            });
        }
        serde_json::from_slice(&finished.stdout)
            .map_err(|error| Failure(format!("The Rust probe answered without JSON: {error}")))
    }

    /// The driver's Wine environment for a probe runtime, with private XDG
    /// directories so that nothing reaches the developer's own profile.
    pub fn isolated_environment(&self, runtime: &Path) -> Result<Environment> {
        let mut environment: Environment =
            serde_json::from_value(self.call("wine-env", json!({"root": host::text(runtime)}))?)?;
        for category in ["CONFIG", "DATA", "CACHE", "STATE"] {
            let path = runtime.join("private/xdg").join(category.to_lowercase());
            host::dirs(&path, host::PRIVATE)?;
            environment.insert(format!("XDG_{category}_HOME"), host::text(&path));
        }
        Ok(environment)
    }
}

/// Called only just after a probe has created its new private runtime.
pub fn mark(runtime: &Path) -> Result<()> {
    host::write(&runtime.join("private/synthetic-probe"), MARKER)
}

/// The discrete GPU (Vulkan device type 2) of a `probe` report, if there is
/// exactly one and it has a device UUID.
pub fn discrete(report: &Value) -> Result<Option<Gpu>> {
    let devices: Vec<&Value> = report["devices"]
        .as_array()
        .ok_or_else(|| Failure("The probe report lists no devices".into()))?
        .iter()
        .filter(|device| device["type"] == 2)
        .collect();
    let [device] = devices[..] else {
        return Ok(None);
    };
    let Some(uuid) = device["device_uuid"]
        .as_str()
        .filter(|uuid| !uuid.is_empty())
    else {
        return Ok(None);
    };
    match (device["name"].as_str(), device["vendor_id"].as_u64()) {
        (Some(name), Some(vendor)) => Ok(Some(Gpu {
            name: name.into(),
            uuid: uuid.into(),
            vendor,
        })),
        _ => fail("The probe report names no GPU or vendor"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probes::fixtures::script;
    use std::{fs, os::unix::fs::MetadataExt};

    #[test]
    fn requests_carry_the_operation_next_to_the_inputs() {
        let request = encode("renderer", json!({"root": "/r", "bundle": null})).expect("request");
        assert_eq!(
            serde_json::from_slice::<Value>(&request).expect("JSON"),
            json!({"operation": "renderer", "root": "/r", "bundle": null})
        );
        assert_eq!(
            encode("probe", Value::Null).expect("request"),
            br#"{"operation":"probe"}"#
        );
        let replaced = encode("probe", json!({"operation": "prefix"})).expect("request");
        assert_eq!(replaced, br#"{"operation":"probe"}"#);
        assert!(encode("probe", json!([1])).is_err());
    }

    #[test]
    fn the_driver_receives_the_request_and_its_answer_is_decoded() {
        let temp = tempfile::tempdir().expect("temp");
        let binary = temp.path().join("runtime-lab");
        script(
            &binary,
            "cat > \"$0.request\"\necho 'selection trace' >&2\nprintf '%s\\n' '{\"devices\":[1,2]}'",
        );
        let lab = Lab::at(binary.clone());
        let trace = temp.path().join("selection.log");
        let answer = lab
            .call_with_stderr("probe", json!({"root": "/r"}), &trace)
            .expect("answer");
        assert_eq!(answer, json!({"devices": [1, 2]}));
        assert_eq!(
            fs::read_to_string(trace).expect("trace"),
            "selection trace\n"
        );
        let sent: Value = serde_json::from_slice(
            &fs::read(temp.path().join("runtime-lab.request")).expect("request"),
        )
        .expect("JSON");
        assert_eq!(sent, json!({"operation": "probe", "root": "/r"}));
        // The driver is started by its real path, as the Python transport did.
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&binary, &link).expect("link");
        assert!(Lab::at(link).call("probe", Value::Null).is_ok());
    }

    #[test]
    fn a_failing_or_missing_driver_is_reported_in_its_own_words() {
        let temp = tempfile::tempdir().expect("temp");
        let binary = temp.path().join("runtime-lab");
        let error = |body: &str| {
            script(&binary, body);
            Lab::at(binary.clone())
                .call("prefix", Value::Null)
                .expect_err(body)
                .0
        };
        assert_eq!(
            error("cat > /dev/null; echo ' Probe prefix already exists. ' >&2; exit 1"),
            "Probe prefix already exists."
        );
        assert_eq!(error("exit 3"), "Rust probe failed");
        assert!(error("echo not-json").starts_with("The Rust probe answered without JSON: "));
        assert_eq!(
            Lab::at(temp.path().join("missing"))
                .call("probe", Value::Null)
                .expect_err("missing")
                .0,
            BUILD_FIRST
        );
        assert_eq!(
            Lab::at(temp.path().to_path_buf())
                .call("probe", Value::Null)
                .expect_err("directory")
                .0,
            BUILD_FIRST
        );
        assert!(Lab::new(temp.path()).binary.ends_with("runtime-lab"));
    }

    #[test]
    fn the_isolated_environment_adds_private_xdg_directories() {
        let temp = tempfile::tempdir().expect("temp");
        let binary = temp.path().join("runtime-lab");
        script(
            &binary,
            "cat > \"$0.request\"\nprintf '%s\\n' '{\"WINEPREFIX\":\"/p\",\"XDG_DATA_HOME\":\"/host\"}'",
        );
        let runtime = temp.path().join("runtime");
        fs::create_dir_all(runtime.join("private")).expect("runtime");
        mark(&runtime).expect("marker");
        assert_eq!(
            fs::read_to_string(runtime.join("private/synthetic-probe")).expect("marker"),
            "Flightdeck isolated test\n"
        );
        let environment = Lab::at(binary)
            .isolated_environment(&runtime)
            .expect("environment");
        assert_eq!(environment["WINEPREFIX"], "/p");
        for part in ["config", "data", "cache", "state"] {
            let path = runtime.join("private/xdg").join(part);
            assert_eq!(
                environment[&format!("XDG_{}_HOME", part.to_uppercase())],
                host::text(&path)
            );
            assert_eq!(
                fs::metadata(&path).expect("directory").mode() & 0o777,
                0o700
            );
        }
        let sent: Value = serde_json::from_slice(
            &fs::read(temp.path().join("runtime-lab.request")).expect("request"),
        )
        .expect("JSON");
        assert_eq!(
            sent,
            json!({"operation": "wine-env", "root": host::text(&runtime)})
        );
    }

    #[test]
    fn only_a_single_discrete_gpu_with_a_uuid_qualifies() {
        let discrete_gpu = json!({"type": 2, "name": "RX", "vendor_id": 4098, "device_uuid": "ab"});
        let integrated =
            json!({"type": 1, "name": "iGPU", "vendor_id": 32902, "device_uuid": "cd"});
        assert_eq!(
            discrete(&json!({"devices": [integrated, discrete_gpu]})).expect("report"),
            Some(Gpu {
                name: "RX".into(),
                uuid: "ab".into(),
                vendor: 0x1002
            })
        );
        for devices in [
            json!([]),
            json!([integrated]),
            json!([discrete_gpu, discrete_gpu]),
            json!([{"type": 2, "name": "RX", "vendor_id": 4098}]),
            json!([{"type": 2, "name": "RX", "vendor_id": 4098, "device_uuid": ""}]),
            json!([{"type": 2, "name": "RX", "vendor_id": 4098, "device_uuid": null}]),
        ] {
            assert_eq!(
                discrete(&json!({"devices": devices})).expect("report"),
                None,
                "{devices}"
            );
        }
        assert!(discrete(&json!({})).is_err());
        assert!(discrete(&json!({"devices": [{"type": 2, "device_uuid": "ab"}]})).is_err());
    }
}
