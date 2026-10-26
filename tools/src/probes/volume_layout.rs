// SPDX-License-Identifier: MIT
//! The maintenance9 3D-copy layout bug, reproduced in an isolated Wine prefix
//! under the Khronos validation layer: the released graphics bundle must show
//! the layout error beyond slice zero, the candidate bundle and both controls
//! with maintenance9 disabled must be free of validation errors, and all four
//! runs must read every pixel back correctly with validation active. Without
//! maintenance9 support nothing is reproduced and the result is inconclusive,
//! not a pass. No game, account or simulator rendering is involved.
use super::{
    Site,
    host::{self, Environment},
    lab::{self, Gpu},
};
use crate::util::{Result, digest, require};
use regex::Regex;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::LazyLock,
};

#[derive(clap::Args)]
pub struct Arguments {
    #[arg(long)]
    runner: PathBuf,
    /// Previously released graphics bundle
    #[arg(long)]
    baseline: PathBuf,
    /// Bundle containing the layout fix
    #[arg(long)]
    bundle: PathBuf,
    /// New directory for the private test prefix and results
    #[arg(long)]
    output: PathBuf,
    /// Directory containing the Khronos validation layer JSON
    #[arg(long)]
    validation_layer_path: Option<PathBuf>,
}

const LIBRARIES: [&str; 5] = [
    "d3d12.dll",
    "d3d12core.dll",
    "dxgi.dll",
    "d3d11.dll",
    "d3d10core.dll",
];
const LAYOUT_ERROR: &str = "VUID-VkCopyImageInfo2-srcImageLayout-00128";
/// Name, whether the candidate bundle is loaded, whether maintenance9 is
/// disabled. The released renderer comes first: it must reproduce the bug.
const CASES: [(&str, bool, bool); 4] = [
    ("released", false, false),
    ("fixed", true, false),
    ("released-without-maintenance9", false, true),
    ("fixed-without-maintenance9", true, true),
];

/// What one run's log shows.
#[derive(Debug, Default, PartialEq)]
struct Findings {
    pixels_correct: bool,
    validation_active: bool,
    validation_errors: BTreeMap<String, u64>,
    volume_layout_error: bool,
}

#[derive(serde::Serialize)]
struct Outcome {
    case: &'static str,
    exit_code: i32,
    pixels_correct: bool,
    validation_active: bool,
    validation_errors: BTreeMap<String, u64>,
    volume_layout_error: bool,
    renderer_sha256: BTreeMap<&'static str, String>,
}

#[derive(serde::Serialize)]
struct Report<'a> {
    status: &'static str,
    scope: &'static str,
    gpu: &'a str,
    nvidia_hardware: bool,
    probe_sha256: String,
    cases: &'a [Outcome],
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Status {
    Pass,
    Fail,
    Inconclusive,
}
impl Status {
    fn name(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Inconclusive => "INCONCLUSIVE",
        }
    }
    fn exit_code(self) -> u8 {
        match self {
            Self::Pass => 0,
            Self::Fail => 1,
            Self::Inconclusive => 2,
        }
    }
}

/// The scrubbed host environment with native renderer libraries, the one
/// GPU, VKD3D's Vulkan debugging and the validation layer for the children.
fn environment(
    mut host: Environment,
    prefix: &Path,
    gpu: &Gpu,
    layer_path: Option<&Path>,
) -> Environment {
    host::set(
        &mut host,
        [
            ("WINEPREFIX", &host::text(prefix)),
            ("WINEESYNC", "0"),
            ("WINEFSYNC", "0"),
            ("WINEDEBUG", "-all"),
            ("WINEDLLOVERRIDES", "d3d12,d3d12core,dxgi,d3d11,d3d10core=n"),
            ("DXVK_FILTER_DEVICE_UUID", &gpu.uuid),
            ("DXVK_LOG_LEVEL", "info"),
            ("VKD3D_DEBUG", "warn"),
            ("VKD3D_CONFIG", "vk_debug"),
            ("VKD3D_SHADER_CACHE_PATH", "0"),
            ("VK_INSTANCE_LAYERS", "VK_LAYER_KHRONOS_validation"),
        ],
    );
    if let Some(layer_path) = layer_path {
        host::set(&mut host, [("VK_LAYER_PATH", &host::text(layer_path))]);
    }
    host
}

/// Pixel readback can succeed despite invalid Vulkan layouts. The layer's own
/// active check is required and its errors are evaluated separately.
fn findings(log: &str) -> Findings {
    static ERROR: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"err:vkd3d-proton:vkd3d_debug_messenger_callback: ((?:VUID-|UNASSIGNED-)[^:\s]+)",
        )
        .expect("pattern")
    });
    static SLICE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:layer [1-3], mip 0|mipLevel = 0, arrayLayer = [1-3])").expect("pattern")
    });
    let mut validation_errors = BTreeMap::new();
    for error in ERROR.captures_iter(log) {
        *validation_errors.entry(error[1].to_string()).or_insert(0) += 1;
    }
    Findings {
        pixels_correct: log.contains("VOLUME_COPY: pixels=2048 mismatches=0"),
        validation_active: log.contains("Validation layers seem to be loaded correctly."),
        volume_layout_error: !validation_errors.is_empty() && SLICE.is_match(log),
        validation_errors,
    }
}

/// Unsupported or disabled maintenance9 must not look like a verified fix:
/// valid runs without any error anywhere are inconclusive.
fn verdict(cases: &[Outcome]) -> Status {
    let valid = cases
        .iter()
        .all(|row| row.exit_code == 0 && row.pixels_correct && row.validation_active);
    let reproduced = cases.first().is_some_and(|released| {
        released.volume_layout_error && released.validation_errors.contains_key(LAYOUT_ERROR)
    });
    let clean = |rows: &[Outcome]| rows.iter().all(|row| row.validation_errors.is_empty());
    if valid && reproduced && cases.get(1..).is_some_and(clean) {
        Status::Pass
    } else if valid && !reproduced && clean(cases) {
        Status::Inconclusive
    } else {
        Status::Fail
    }
}

pub fn run(site: &Site, arguments: &Arguments) -> Result<u8> {
    let (root, lab) = (site.root, &site.lab);
    let runner = host::canonical(&arguments.runner)?;
    let baseline = host::canonical(&arguments.baseline)?;
    let bundle = host::canonical(&arguments.bundle)?;
    let layer_path = arguments
        .validation_layer_path
        .as_deref()
        .map(host::canonical)
        .transpose()?;
    let output = host::resolve(&arguments.output)?;
    host::new_dirs(&output, host::DEFAULT)?;
    let runtime = output.join("runtime");
    host::new_dirs(&runtime.join("private"), host::DEFAULT)?;
    host::new_dir(&runtime.join("local"), host::DEFAULT)?;
    std::os::unix::fs::symlink(&runner, runtime.join("runner"))?;
    let prefix = runtime.join("local/msfs-prefix");
    let Some(gpu) = lab::discrete(&lab.call("probe", serde_json::Value::Null)?)? else {
        return super::refused("Requires one discrete Vulkan GPU with a device UUID");
    };
    let source = root.join("tests/graphics/volume-copy.c");
    let binary = output.join("volume-copy.exe");
    host::checked(
        Command::new(&site.compiler)
            .args(["-O2", "-Wall", "-Wextra", "-Werror"])
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .args(["-ld3d12", "-ldxgi", "-ldxguid"]),
    )?;
    lab::mark(&runtime)?;
    let root_input = host::text(&runtime);
    lab.call("prefix", json!({"root": root_input}))?;
    let environment = environment(
        host::scrubbed(std::env::vars_os()),
        &prefix,
        &gpu,
        layer_path.as_deref(),
    );
    let wineserver = runner.join("files/bin/wineserver");
    let mut outcomes = Vec::new();
    for (name, candidate, disable) in CASES {
        let payload = if candidate { &bundle } else { &baseline };
        let renderer = lab.call(
            "renderer",
            json!({"root": root_input, "bundle": host::text(payload)}),
        )?;
        require(renderer == "backport", "Bundle does not match the runner")?;
        let mut env = environment.clone();
        if disable {
            host::set(
                &mut env,
                [("VKD3D_DISABLE_EXTENSIONS", "VK_KHR_maintenance9")],
            );
        }
        let log_path = output.join(format!("{name}.log"));
        let ran = host::logged(
            Command::new(runner.join("files/bin/wine"))
                .arg(&binary)
                .current_dir(&output)
                .env_clear()
                .envs(&env),
            &log_path,
            60,
        )
        .and_then(|exit_code| {
            let found = findings(&String::from_utf8_lossy(&fs::read(&log_path)?));
            let mut renderer_sha256 = BTreeMap::new();
            for library in LIBRARIES {
                renderer_sha256.insert(library, digest(&fs::read(payload.join(library))?));
            }
            Ok(Outcome {
                case: name,
                exit_code,
                pixels_correct: found.pixels_correct,
                validation_active: found.validation_active,
                validation_errors: found.validation_errors,
                volume_layout_error: found.volume_layout_error,
                renderer_sha256,
            })
        });
        let stopped = host::stop_wine(&wineserver, &env, |command| {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        });
        outcomes.push(host::finally(ran, stopped)?);
    }
    let status = verdict(&outcomes);
    let report = host::pretty(&Report {
        status: status.name(),
        scope: "3D texture upload/copy/readback with Vulkan layout validation",
        gpu: &gpu.name,
        nvidia_hardware: gpu.vendor == lab::NVIDIA,
        probe_sha256: digest(&fs::read(&source)?),
        cases: &outcomes,
    })?;
    host::write(&output.join("result.json"), &report)?;
    print!("{report}");
    Ok(status.exit_code())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const ACTIVE: &str = "info:vkd3d-proton: Validation layers seem to be loaded correctly.\n";
    const PIXELS: &str = "VOLUME_COPY: pixels=2048 mismatches=0\n";

    fn layout_error(detail: &str) -> String {
        format!(
            "err:vkd3d-proton:vkd3d_debug_messenger_callback: {LAYOUT_ERROR}: Validation Error: {detail}\n"
        )
    }

    fn outcome(case: &'static str, exit_code: i32, log: &str) -> Outcome {
        let found = findings(log);
        Outcome {
            case,
            exit_code,
            pixels_correct: found.pixels_correct,
            validation_active: found.validation_active,
            validation_errors: found.validation_errors,
            volume_layout_error: found.volume_layout_error,
            renderer_sha256: BTreeMap::new(),
        }
    }

    /// Four runs from the log of the released renderer and of the other three.
    fn runs(released: &str, others: &str) -> Vec<Outcome> {
        CASES
            .iter()
            .enumerate()
            .map(|(index, (name, ..))| outcome(name, 0, if index == 0 { released } else { others }))
            .collect()
    }

    #[test]
    fn the_environment_enables_validation_for_the_one_gpu() {
        let gpu = Gpu {
            name: "GPU".into(),
            uuid: "ab".repeat(16),
            vendor: 0x1002,
        };
        let host = || Environment::from([("PATH".to_string(), "/usr/bin".to_string())]);
        let plain = environment(host(), Path::new("/out/prefix"), &gpu, None);
        assert_eq!(
            plain,
            [
                ("DXVK_FILTER_DEVICE_UUID", "ab".repeat(16).as_str()),
                ("DXVK_LOG_LEVEL", "info"),
                ("PATH", "/usr/bin"),
                ("VKD3D_CONFIG", "vk_debug"),
                ("VKD3D_DEBUG", "warn"),
                ("VKD3D_SHADER_CACHE_PATH", "0"),
                ("VK_INSTANCE_LAYERS", "VK_LAYER_KHRONOS_validation"),
                ("WINEDEBUG", "-all"),
                ("WINEDLLOVERRIDES", "d3d12,d3d12core,dxgi,d3d11,d3d10core=n"),
                ("WINEESYNC", "0"),
                ("WINEFSYNC", "0"),
                ("WINEPREFIX", "/out/prefix"),
            ]
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .into()
        );
        let layered = environment(host(), Path::new("/p"), &gpu, Some(Path::new("/layers")));
        assert_eq!(layered["VK_LAYER_PATH"], "/layers");
        assert_eq!(layered.len(), plain.len() + 1);
    }

    #[test]
    fn the_cases_compare_both_bundles_with_and_without_maintenance9() {
        assert_eq!(
            CASES,
            [
                ("released", false, false),
                ("fixed", true, false),
                ("released-without-maintenance9", false, true),
                ("fixed-without-maintenance9", true, true),
            ]
        );
    }

    #[test]
    fn logs_are_read_for_pixels_the_active_layer_and_counted_errors() {
        let log = format!(
            "{ACTIVE}{}{}err:vkd3d-proton:vkd3d_debug_messenger_callback: UNASSIGNED-CoreValidation-Shader: x\n\
             err:vkd3d-proton:vkd3d_debug_messenger_callback: Something else: y\n{PIXELS}",
            layout_error("layer 2, mip 0"),
            layout_error("layer 3, mip 0"),
        );
        assert_eq!(
            findings(&log),
            Findings {
                pixels_correct: true,
                validation_active: true,
                validation_errors: [
                    (LAYOUT_ERROR.to_string(), 2),
                    ("UNASSIGNED-CoreValidation-Shader".to_string(), 1)
                ]
                .into(),
                volume_layout_error: true,
            }
        );
        // The newer layer wording names the same slices differently.
        assert!(findings(&layout_error("mipLevel = 0, arrayLayer = 1")).volume_layout_error);
        // Slice zero or another mip level is not this bug.
        assert!(!findings(&layout_error("layer 0, mip 0")).volume_layout_error);
        assert!(!findings(&layout_error("layer 4, mip 0")).volume_layout_error);
        assert!(!findings(&layout_error("mipLevel = 1, arrayLayer = 1")).volume_layout_error);
        // A slice mention without any validation error proves nothing.
        assert_eq!(
            findings("trace: layer 1, mip 0\nVOLUME_COPY: pixels=2048 mismatches=3\n"),
            Findings::default()
        );
    }

    #[test]
    fn a_pass_needs_the_reproduced_error_and_three_clean_runs() {
        let valid = format!("{ACTIVE}{PIXELS}");
        let reproduced = format!("{valid}{}", layout_error("layer 1, mip 0"));
        assert_eq!(verdict(&runs(&reproduced, &valid)), Status::Pass);
        // Without the error anywhere, maintenance9 was not exercised.
        assert_eq!(verdict(&runs(&valid, &valid)), Status::Inconclusive);
        // The candidate still shows an error.
        assert_eq!(verdict(&runs(&reproduced, &reproduced)), Status::Fail);
        // An error in a later run without reproduction is no inconclusive result.
        assert_eq!(verdict(&runs(&valid, &reproduced)), Status::Fail);
        // The released renderer shows the VUID, but not beyond slice zero.
        let slice_zero = format!("{valid}{}", layout_error("layer 0, mip 0"));
        assert_eq!(verdict(&runs(&slice_zero, &valid)), Status::Fail);
        // Another error on the right slices is not the layout bug.
        let other = format!(
            "{valid}err:vkd3d-proton:vkd3d_debug_messenger_callback: VUID-other: layer 1, mip 0\n"
        );
        assert_eq!(verdict(&runs(&other, &valid)), Status::Fail);
    }

    #[test]
    fn an_invalid_run_fails_whatever_the_errors_say() {
        let valid = format!("{ACTIVE}{PIXELS}");
        let reproduced = format!("{valid}{}", layout_error("layer 1, mip 0"));
        for broken in [ACTIVE, PIXELS, ""] {
            assert_eq!(
                verdict(&runs(&reproduced, broken)),
                Status::Fail,
                "{broken}"
            );
            assert_eq!(verdict(&runs(broken, &valid)), Status::Fail, "{broken}");
        }
        let mut crashed = runs(&reproduced, &valid);
        crashed[3].exit_code = 1;
        assert_eq!(verdict(&crashed), Status::Fail);
        let mut silent = runs(&valid, &valid);
        silent[0].exit_code = -11;
        assert_eq!(verdict(&silent), Status::Fail);
    }

    #[test]
    fn the_status_decides_the_exit_code() {
        assert_eq!(
            [Status::Pass, Status::Fail, Status::Inconclusive].map(|s| (s.name(), s.exit_code())),
            [("PASS", 0), ("FAIL", 1), ("INCONCLUSIVE", 2)]
        );
    }

    #[test]
    fn the_result_keeps_the_keys_of_the_python_report() {
        let mut row = outcome("released", 0, &layout_error("layer 1, mip 0"));
        row.renderer_sha256 = LIBRARIES.map(|name| (name, digest(name.as_bytes()))).into();
        let report = host::pretty(&Report {
            status: "FAIL",
            scope: "3D texture upload/copy/readback with Vulkan layout validation",
            gpu: "GPU",
            nvidia_hardware: false,
            probe_sha256: digest(b""),
            cases: &[row],
        })
        .expect("report");
        let value: Value = serde_json::from_str(&report).expect("JSON");
        let mut keys = value
            .as_object()
            .expect("object")
            .keys()
            .collect::<Vec<_>>();
        keys.sort();
        assert_eq!(
            keys,
            [
                "cases",
                "gpu",
                "nvidia_hardware",
                "probe_sha256",
                "scope",
                "status"
            ]
        );
        assert_eq!(
            value["cases"][0],
            json!({"case": "released", "exit_code": 0, "pixels_correct": false,
                "validation_active": false, "validation_errors": {LAYOUT_ERROR: 1},
                "volume_layout_error": true,
                "renderer_sha256": LIBRARIES.iter().map(|name| (name.to_string(), digest(name.as_bytes())))
                    .collect::<BTreeMap<_, _>>()})
        );
        assert!(report.starts_with("{\n  \"status\": \"FAIL\",\n  \"scope\""));
    }
}
