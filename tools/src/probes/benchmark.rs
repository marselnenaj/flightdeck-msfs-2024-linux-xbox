// SPDX-License-Identifier: MIT
//! The Rust launcher against the retired Python launcher: service startup,
//! resident memory and API latency of fresh, unconfigured services, and the
//! wall time of decoding identical synthetic save states. The Python side is
//! the subject of the comparison, so an interpreter and an exported checkout
//! of the Python revision are required; its own code also writes the save
//! workloads, which keeps them byte-identical to earlier measurements.
//! Nothing here starts Wine or a game, uses an account or the network, or
//! measures the desktop, simulator frame rates or flight loading.
use crate::{
    ensure, http,
    probes::host,
    util::{Failure, Result, digest, fail, require},
};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::Write,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::LazyLock,
    time::{Duration, Instant},
};

#[derive(clap::Args)]
pub struct Arguments {
    #[arg(long)]
    binary: PathBuf,
    #[arg(long)]
    python_source: PathBuf,
    #[arg(long)]
    python_commit: String,
    #[arg(long)]
    rust_launcher: Option<PathBuf>,
    #[arg(long)]
    python_launcher: Option<PathBuf>,
    #[arg(long)]
    output: PathBuf,
    #[arg(long, default_value_t = 7)]
    repeats: u32,
    #[arg(long, default_value_t = 50)]
    requests: u32,
    /// Interpreter for the Python side; the script used the one running it
    #[arg(long, default_value = "python3")]
    python: PathBuf,
}

const PATHS: [&str; 4] = [
    "/api/status",
    "/api/setup",
    "/api/cloud-saves",
    "/api/launcher-update",
];
/// Label, number of blobs and bytes per blob of each save workload.
const WORKLOADS: [(&str, u32, u32); 3] = [
    ("small_128_kib", 32, 4096),
    ("many_4096_blobs", 4096, 256),
    ("large_32_mib", 32, 1024 * 1024),
];
const METHOD: &str = "alternating fresh processes after one discarded warm-up; warm filesystem cache; no game, account, cloud network or Wine";
const SCOPE: &str = "launcher service only; no GUI, simulator FPS or flight-loading measurement";

/// Fails early when the checkout is not the Python launcher, and names the
/// interpreter in the report.
const PYTHON_VERSION: &str = "import sys
sys.path.insert(0,sys.argv[1])
from flightdeck import save_state
print(sys.version.split()[0])
";
/// Writes the last of the given workloads. The earlier ones are drawn from
/// the same generator first, so each file has the bytes it always had.
const PYTHON_WORKLOAD: &str = "import random,sys
sys.path.insert(0,sys.argv[1])
from flightdeck import save_state
rng=random.Random(20261003)
for spec in sys.argv[3:]:
    count,size=map(int,spec.split(':'))
    blobs={f'blob{i:05d}':rng.randbytes(size) for i in range(count)}
state=save_state.State(42,{'synthetic':save_state.Container('Synthetic benchmark',1,blobs)})
open(sys.argv[2],'wb').write(save_state.encode(state))
";
const PYTHON_SAVE: &str = "import hashlib,json,sys
sys.path.insert(0,sys.argv[1])
from flightdeck import save_state
raw=open(sys.argv[2],'rb').read()
state=save_state.decode(raw)
print(json.dumps({'canonical_sha256':hashlib.sha256(save_state.encode(state)).hexdigest(),
 'content_sha256':save_state.content_digest(state),'containers':len(state.containers),
 'blobs':sum(len(c.blobs) for c in state.containers.values()),
 'bytes':sum(len(b) for c in state.containers.values() for b in c.blobs.values())}))
";

type Launchers = [(&'static str, Vec<OsString>); 2];

/// One measured service start.
struct Run {
    startup_ms: f64,
    memory: BTreeMap<String, u32>,
    api_ms: BTreeMap<&'static str, Vec<f64>>,
}

/// Median, range and the p95 observation as Python's `statistics` gives
/// them: whole numbers stay whole unless two middle values are averaged.
fn summary<T: Copy + PartialOrd + Into<f64> + Into<Value>>(samples: &[T]) -> Result<Value> {
    let mut ordered = samples.to_vec();
    ordered.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let (Some(&min), Some(&max)) = (ordered.first(), ordered.last()) else {
        return fail("No samples were measured");
    };
    let middle = ordered.len() / 2;
    let median: Value = if ordered.len() % 2 == 1 {
        ordered[middle].into()
    } else {
        let (low, high): (f64, f64) = (ordered[middle - 1].into(), ordered[middle].into());
        ((low + high) / 2.0).into()
    };
    let p95 = ordered[(ordered.len() - 1).min((ordered.len() as f64 * 0.95) as usize)];
    let mut result = json!({"samples": ordered.len(), "median": median, "min": Into::<Value>::into(min),
        "p95": Into::<Value>::into(p95), "max": Into::<Value>::into(max)});
    result.sort_all_objects();
    Ok(result)
}

/// Rust first on odd repeats and Python first on even ones, so neither
/// always runs in the other's wake. Repeat zero is the discarded warm-up.
fn order(repeat: u32) -> [usize; 2] {
    if repeat % 2 == 1 { [0, 1] } else { [1, 0] }
}

/// Resident and peak resident memory from a `/proc/<pid>/status` text.
fn memory(status: &str) -> BTreeMap<String, u32> {
    status
        .lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(key, _)| ["VmRSS", "VmHWM"].contains(key))
        .filter_map(|(key, value)| {
            Some((
                format!("{key}_kib"),
                value.split_whitespace().next()?.parse().ok()?,
            ))
        })
        .collect()
}

/// The processor name and the number of logical processors.
fn processor(cpuinfo: &str) -> (String, usize) {
    let name = cpuinfo
        .lines()
        .find(|line| line.starts_with("model name"))
        .and_then(|line| line.split_once(':'))
        .map_or("unknown", |(_, name)| name.trim());
    let count = cpuinfo
        .lines()
        .filter(|line| {
            line.split_once(':')
                .is_some_and(|(key, _)| key.trim() == "processor")
        })
        .count();
    (name.to_string(), count)
}

/// A private home for every started process, and the Python checkout on the
/// module path. The remaining host environment is inherited.
fn environment(root: &Path, python_source: &Path) -> Vec<(String, OsString)> {
    let mut environment = vec![("HOME".to_string(), root.join("home").into_os_string())];
    for key in ["CONFIG", "DATA", "CACHE", "STATE"] {
        environment.push((
            format!("XDG_{key}_HOME"),
            root.join(key.to_lowercase()).into_os_string(),
        ));
    }
    environment.push(("PYTHONPATH".into(), python_source.into()));
    environment.push(("PYTHONUNBUFFERED".into(), "1".into()));
    environment
}

/// The launchers by mode: always the built binary against the Python module,
/// and the two installed entry points when both are given.
fn commands(
    python: &Path,
    binary: &Path,
    installed: Option<(PathBuf, PathBuf)>,
) -> Vec<(&'static str, Launchers)> {
    let words = |words: &[&Path]| {
        words
            .iter()
            .map(|word| word.as_os_str().to_owned())
            .collect()
    };
    let mut commands = vec![(
        "direct",
        [
            ("rust", words(&[binary])),
            (
                "python",
                words(&[python, Path::new("-m"), Path::new("flightdeck")]),
            ),
        ],
    )];
    if let Some((rust, script)) = installed {
        commands.push((
            "installed",
            [
                ("rust", words(&[&rust])),
                ("python", words(&[python, &script])),
            ],
        ));
    }
    commands
}

/// One GET on a new connection: the time until the whole answer is read, in
/// milliseconds, and the answer. The client asks the service to close the
/// connection, so its closing is part of the measured time.
fn request(port: u16, path: &str, token: Option<&str>) -> Result<(f64, Value)> {
    let mut headers = vec![("Accept-Language", "en")];
    headers.extend(token.map(|token| ("X-Flightdeck-Token", token)));
    let start = Instant::now();
    let response = http::request(port, "GET", path, &headers, None, Duration::from_secs(30))?;
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    ensure!(response.status == 200, "{path}");
    Ok((elapsed, serde_json::from_slice(&response.body)?))
}

fn measure(child: &mut Child, directory: &Path, started: Instant, samples: u32) -> Result<Run> {
    static ADDRESS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"http://127\.0\.0\.1:(\d+)").expect("pattern"));
    let line = host::first_line(child, 30, "Service did not start")?;
    let port = ADDRESS
        .captures(&line)
        .and_then(|found| found[1].parse().ok())
        .ok_or_else(|| Failure(format!("Unexpected service startup output: {line}")))?;
    let record = directory.join("state/desktop-service.json");
    // The Python launcher writes no service record and needs no token.
    let token = if record.exists() {
        host::read_json(&record)?["token"]
            .as_str()
            .filter(|token| !token.is_empty())
            .map(str::to_string)
    } else {
        None
    };
    let token = token.as_deref();
    let (_, state) = request(port, "/api/status", token)?;
    ensure!(
        !host::truthy(&state["runtime"]["configured"])
            && !host::truthy(&state["game"]["can_start"])
    );
    let startup_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut api_ms = BTreeMap::new();
    for path in PATHS {
        for _ in 0..5 {
            request(port, path, token)?;
        }
        let mut timings = Vec::new();
        for _ in 0..samples {
            timings.push(request(port, path, token)?.0);
        }
        api_ms.insert(path, timings);
    }
    Ok(Run {
        startup_ms,
        memory: memory(&fs::read_to_string(format!("/proc/{}/status", child.id()))?),
        api_ms,
    })
}

/// Starts one launcher in its own directory and process group, measures it
/// and interrupts the whole group. The group replaces the Python original's
/// new session, which would need the unsafe `pre_exec`; the signal reaches
/// the same processes.
fn service(
    command: &[OsString],
    directory: &Path,
    environment: &[(String, OsString)],
    samples: u32,
) -> Result<Run> {
    let (program, arguments) = command
        .split_first()
        .ok_or_else(|| Failure("Empty launcher command".into()))?;
    let started = Instant::now();
    let mut child = Command::new(program)
        .args(arguments)
        .arg("--state-dir")
        .arg(directory.join("state"))
        .arg("--no-browser")
        .current_dir(directory)
        .envs(environment.iter().map(|(name, value)| (name, value)))
        .stdout(Stdio::piped())
        // Never read, as in the original: a very talkative launcher blocks.
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|error| Failure(format!("{}: {error}", program.to_string_lossy())))?;
    let measured = measure(&mut child, directory, started, samples);
    let group = rustix::process::Pid::from_raw(child.id() as i32);
    let signal = |signal| {
        if let Some(group) = group {
            let _ = rustix::process::kill_process_group(group, signal);
        }
    };
    signal(rustix::process::Signal::INT);
    if !host::exited_within(&mut child, 30) {
        signal(rustix::process::Signal::KILL);
        let _ = child.wait();
    }
    measured
}

/// Summaries of the measured runs of one launcher.
fn service_summary(runs: &[Run]) -> Result<Value> {
    let memory = |key: &str| {
        runs.iter()
            .map(|run| {
                run.memory
                    .get(key)
                    .copied()
                    .ok_or_else(|| Failure(format!("The service status has no {key}")))
            })
            .collect::<Result<Vec<u32>>>()
    };
    let mut api_ms = Map::new();
    for path in PATHS {
        let timings: Vec<f64> = runs
            .iter()
            .flat_map(|run| run.api_ms.get(path).into_iter().flatten().copied())
            .collect();
        api_ms.insert(path.into(), summary(&timings)?);
    }
    Ok(json!({
        "startup_ms": summary(&runs.iter().map(|run| run.startup_ms).collect::<Vec<_>>())?,
        "rss_kib": summary(&memory("VmRSS_kib")?)?,
        "peak_rss_kib": summary(&memory("VmHWM_kib")?)?,
        "api_ms": api_ms,
    }))
}

/// Decodes one save file with both implementations, alternating, and
/// requires identical results before any time counts.
fn saves(
    commands: &Launchers,
    path: &Path,
    environment: &[(String, OsString)],
    repeats: u32,
) -> Result<Value> {
    let mut wall = [Vec::new(), Vec::new()];
    let mut expected = None;
    for repeat in 0..=repeats {
        for index in order(repeat) {
            let (name, command) = &commands[index];
            let start = Instant::now();
            let finished = host::run(
                Command::new(&command[0])
                    .args(&command[1..])
                    .envs(environment.iter().map(|(name, value)| (name, value)))
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped()),
                None,
                120,
            )?;
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            require(
                finished.code == 0,
                format!(
                    "The {name} save check failed with exit status {}: {}",
                    finished.code,
                    String::from_utf8_lossy(&finished.stderr).trim()
                ),
            )?;
            let mut value: Value = serde_json::from_slice(&finished.stdout)?;
            if let Some(result) = value.as_object_mut() {
                result.remove("valid");
            }
            let expected = expected.get_or_insert_with(|| value.clone());
            require(value == *expected, "Save workload results differ")?;
            if repeat > 0 {
                wall[index].push(elapsed);
            }
        }
    }
    let mut process_wall_ms = Map::new();
    for ((name, _), values) in commands.iter().zip(&wall) {
        process_wall_ms.insert((*name).into(), summary(values)?);
    }
    Ok(
        json!({"bytes": fs::metadata(path)?.len(), "identical_results": true,
        "process_wall_ms": process_wall_ms}),
    )
}

/// Runs a short program with the Python checkout and returns what it prints.
fn python(interpreter: &Path, program: &str, arguments: &[&std::ffi::OsStr]) -> Result<String> {
    let result = Command::new(interpreter)
        .arg("-c")
        .arg(program)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| Failure(format!("{}: {error}", interpreter.display())))?;
    require(
        result.status.success(),
        format!(
            "{} failed: {}",
            interpreter.display(),
            String::from_utf8_lossy(&result.stderr).trim()
        ),
    )?;
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_string())
}

pub fn run(arguments: &Arguments) -> Result<u8> {
    // Without a measured run there is nothing to summarize.
    require(
        arguments.repeats >= 1 && arguments.requests >= 1,
        "--repeats and --requests must be at least 1",
    )?;
    let binary = host::canonical(&arguments.binary)?;
    let python_source = host::canonical(&arguments.python_source)?;
    let interpreter = arguments.python.as_path();
    let version = python(interpreter, PYTHON_VERSION, &[python_source.as_os_str()])?;
    let temporary = tempfile::Builder::new()
        .prefix("flightdeck-benchmark-")
        .tempdir()?;
    let root = temporary.path();
    host::new_dir(&root.join("home"), host::DEFAULT)?;
    let environment = environment(root, &python_source);
    let installed = match (&arguments.rust_launcher, &arguments.python_launcher) {
        (Some(rust), Some(script)) => Some((host::resolve(rust)?, host::resolve(script)?)),
        _ => None,
    };
    let commands = commands(interpreter, &binary, installed);
    let mut services = Map::new();
    for (mode, launchers) in &commands {
        let mut runs = [Vec::new(), Vec::new()];
        for repeat in 0..=arguments.repeats {
            for index in order(repeat) {
                let (name, command) = &launchers[index];
                let directory = root.join(format!("{mode}-{repeat}-{name}"));
                host::new_dir(&directory, host::DEFAULT)?;
                let value = service(command, &directory, &environment, arguments.requests)?;
                // An explicit discarded warm-up for each launcher.
                if repeat > 0 {
                    runs[index].push(value);
                }
            }
        }
        let mut summaries = Map::new();
        for ((name, _), runs) in launchers.iter().zip(&runs) {
            summaries.insert((*name).into(), service_summary(runs)?);
        }
        services.insert((*mode).into(), summaries.into());
    }
    let save_commands: Launchers = [
        ("rust", vec![binary.clone().into(), "save-check".into()]),
        (
            "python",
            vec![
                interpreter.into(),
                "-c".into(),
                PYTHON_SAVE.into(),
                python_source.clone().into(),
            ],
        ),
    ];
    let mut workloads = Map::new();
    for (index, (label, ..)) in WORKLOADS.iter().enumerate() {
        let path = root.join(format!("{label}.bin"));
        let specifications: Vec<OsString> = WORKLOADS[..=index]
            .iter()
            .map(|(_, count, size)| format!("{count}:{size}").into())
            .collect();
        let mut generator = vec![python_source.as_os_str(), path.as_os_str()];
        generator.extend(specifications.iter().map(OsString::as_os_str));
        python(interpreter, PYTHON_WORKLOAD, &generator)?;
        let with_path = save_commands.clone().map(|(name, mut command)| {
            command.push(path.clone().into());
            (name, command)
        });
        workloads.insert(
            (*label).into(),
            saves(&with_path, &path, &environment, arguments.repeats)?,
        );
    }
    let (cpu, logical_cpus) = processor(&fs::read_to_string("/proc/cpuinfo")?);
    let report = json!({"schema": 1, "method": METHOD, "scope": SCOPE, "python": version,
        "python_source_commit": arguments.python_commit, "rust_binary_sha256": digest(&fs::read(&binary)?),
        "machine": {"cpu": cpu, "kernel": fs::read_to_string("/proc/sys/kernel/osrelease")?.trim(),
            "logical_cpus": logical_cpus},
        "services": services, "saves": workloads});
    if let Some(parent) = arguments
        .output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        host::dirs(parent, host::DEFAULT)?;
    }
    // The report must be new; an earlier measurement is never replaced.
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&arguments.output)
        .and_then(|mut stream| stream.write_all(&crate::util::pretty(&report)))
        .map_err(|error| Failure(format!("{}: {error}", arguments.output.display())))?;
    println!(
        "{}",
        json!({"status": "PASS", "report": host::text(&arguments.output),
            "modes": commands.iter().map(|(mode, _)| *mode).collect::<Vec<_>>(),
            "workloads": WORKLOADS.map(|(label, ..)| label)})
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{http::fixtures::Service, probes::fixtures::script};

    #[test]
    fn summaries_match_pythons_statistics() {
        assert_eq!(
            summary(&[3.0, 1.0, 2.0]).expect("odd"),
            json!({"samples": 3, "median": 2.0, "min": 1.0, "p95": 3.0, "max": 3.0})
        );
        assert_eq!(
            summary(&[4.0, 1.0, 2.0, 3.0]).expect("even"),
            json!({"samples": 4, "median": 2.5, "min": 1.0, "p95": 4.0, "max": 4.0})
        );
        // Whole kibibytes stay whole unless two middle values are averaged.
        assert_eq!(
            summary(&[300u32, 100, 200]).expect("whole").to_string(),
            r#"{"max":300,"median":200,"min":100,"p95":300,"samples":3}"#
        );
        assert_eq!(summary(&[100u32, 201]).expect("averaged")["median"], 150.5);
        // p95 is the observation at index int(n * 0.95), capped at the last.
        let hundred: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(summary(&hundred).expect("hundred")["p95"], 96.0);
        let twenty: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(summary(&twenty).expect("twenty")["p95"], 20.0);
        assert_eq!(summary(&[7.5]).expect("single")["p95"], 7.5);
        assert!(summary::<f64>(&[]).is_err());
    }

    #[test]
    fn the_launchers_alternate_and_the_warm_up_starts_with_python() {
        assert_eq!([0, 1, 2, 3].map(order), [[1, 0], [0, 1], [1, 0], [0, 1]]);
    }

    #[test]
    fn memory_is_read_from_the_process_status() {
        let status = "Name:\tflightdeck\nVmPeak:\t  999 kB\nVmHWM:\t   52340 kB\nVmRSS:\t   48112 kB\nThreads:\t9\n";
        assert_eq!(
            memory(status),
            [
                ("VmHWM_kib".to_string(), 52340),
                ("VmRSS_kib".to_string(), 48112)
            ]
            .into()
        );
        assert!(memory("Name:\tkernel-thread\n").is_empty());
    }

    #[test]
    fn the_machine_is_described_from_cpuinfo() {
        let cpuinfo = "processor\t: 0\nmodel name\t: AMD Ryzen 9 7950X3D 16-Core Processor\n\nprocessor\t: 1\nmodel name\t: AMD Ryzen 9 7950X3D 16-Core Processor\n";
        assert_eq!(
            processor(cpuinfo),
            ("AMD Ryzen 9 7950X3D 16-Core Processor".to_string(), 2)
        );
        assert_eq!(processor("processor\t: 0\n"), ("unknown".to_string(), 1));
    }

    #[test]
    fn every_process_gets_a_private_home_and_the_python_checkout() {
        assert_eq!(
            environment(Path::new("/tmp/bench"), Path::new("/src/python")),
            [
                ("HOME", "/tmp/bench/home"),
                ("XDG_CONFIG_HOME", "/tmp/bench/config"),
                ("XDG_DATA_HOME", "/tmp/bench/data"),
                ("XDG_CACHE_HOME", "/tmp/bench/cache"),
                ("XDG_STATE_HOME", "/tmp/bench/state"),
                ("PYTHONPATH", "/src/python"),
                ("PYTHONUNBUFFERED", "1"),
            ]
            .map(|(name, value)| (name.to_string(), OsString::from(value)))
        );
    }

    #[test]
    fn installed_launchers_are_measured_only_when_both_are_given() {
        let words = |words: &[&str]| words.iter().map(OsString::from).collect::<Vec<_>>();
        let direct = commands(Path::new("python3"), Path::new("/bin/rust"), None);
        assert_eq!(
            direct,
            [(
                "direct",
                [
                    ("rust", words(&["/bin/rust"])),
                    ("python", words(&["python3", "-m", "flightdeck"]))
                ]
            )]
        );
        let both = commands(
            Path::new("python3"),
            Path::new("/bin/rust"),
            Some((
                "/opt/rust/flightdeck".into(),
                "/opt/python/flightdeck".into(),
            )),
        );
        assert_eq!(both[0], direct[0]);
        assert_eq!(
            both[1],
            (
                "installed",
                [
                    ("rust", words(&["/opt/rust/flightdeck"])),
                    ("python", words(&["python3", "/opt/python/flightdeck"]))
                ]
            )
        );
    }

    #[test]
    fn the_workloads_and_the_python_programs_are_those_of_the_script() {
        assert_eq!(
            WORKLOADS,
            [
                ("small_128_kib", 32, 4096),
                ("many_4096_blobs", 4096, 256),
                ("large_32_mib", 32, 1_048_576)
            ]
        );
        assert!(PYTHON_WORKLOAD.contains("random.Random(20261003)"));
        assert!(PYTHON_WORKLOAD.contains("save_state.Container('Synthetic benchmark',1,blobs)"));
        assert!(PYTHON_SAVE.contains("save_state.content_digest(state)"));
    }

    /// A launcher that announces the fixture service and waits to be stopped.
    fn launcher(directory: &Path, port: u16, record: bool) -> Vec<OsString> {
        let binary = directory.join("launcher");
        let record = if record {
            "mkdir -p \"$2\"\nprintf '%s' '{\"token\":\"session\"}' > \"$2/desktop-service.json\"\n"
        } else {
            ""
        };
        script(
            &binary,
            &format!(
                "{record}printf '%s\\n' \"$PWD\" \"$HOME\" \"$PYTHONUNBUFFERED\" > started\n\
                 echo \"Flightdeck: http://127.0.0.1:{port}\"\nexec sleep 60"
            ),
        );
        vec![binary.into()]
    }

    #[test]
    fn a_service_is_measured_in_its_own_directory_and_then_interrupted() {
        let service_fixture = Service::start(|_, path, headers, _| {
            let token = headers
                .get("x-flightdeck-token")
                .map(|values| values[0].as_str());
            let known = PATHS.contains(&path);
            match token {
                Some("session") | None if known => (
                    200,
                    json!({"runtime": {"configured": false}, "game": {"can_start": false},
                        "authenticated": token.is_some()}),
                ),
                _ => (404, json!({})),
            }
        });
        let temp = tempfile::tempdir().expect("temp");
        let root = fs::canonicalize(temp.path()).expect("root");
        let environment = environment(&root, Path::new("/src/python"));
        for (name, record) in [("rust", true), ("python", false)] {
            let directory = root.join(name);
            fs::create_dir(&directory).expect("directory");
            let command = launcher(&directory, service_fixture.port, record);
            let run = service(&command, &directory, &environment, 3).expect(name);
            assert!(run.startup_ms > 0.0);
            assert!(
                run.memory["VmRSS_kib"] > 0 && run.memory["VmHWM_kib"] >= run.memory["VmRSS_kib"]
            );
            assert_eq!(run.api_ms.len(), 4);
            assert!(run.api_ms.values().all(|timings| timings.len() == 3));
            assert_eq!(
                fs::read_to_string(directory.join("started")).expect("started"),
                format!(
                    "{}\n{}\n1\n",
                    directory.display(),
                    root.join("home").display()
                )
            );
            let summarized = service_summary(&[run]).expect("summary");
            assert_eq!(summarized["startup_ms"]["samples"], 1);
            assert_eq!(summarized["api_ms"]["/api/setup"]["samples"], 3);
            assert!(summarized["rss_kib"]["median"].is_u64());
            assert!(summarized["peak_rss_kib"]["max"].is_u64());
        }
    }

    #[test]
    fn a_configured_or_silent_service_is_not_measured() {
        let configured = Service::start(|_, _, _, _| {
            (
                200,
                json!({"runtime": {"configured": true}, "game": {"can_start": false}}),
            )
        });
        let temp = tempfile::tempdir().expect("temp");
        let command = launcher(temp.path(), configured.port, false);
        assert!(service(&command, temp.path(), &[], 1).is_err());
        let silent = temp.path().join("silent");
        script(&silent, "echo 'no address here'\nexec sleep 60");
        let error = service(&[silent.into()], temp.path(), &[], 1)
            .err()
            .expect("silent");
        assert_eq!(
            error.0,
            "Unexpected service startup output: no address here\n"
        );
        assert!(service(&[], temp.path(), &[], 1).is_err());
    }

    #[test]
    fn save_results_must_be_identical_before_any_time_counts() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("workload.bin");
        fs::write(&path, [0u8; 64]).expect("workload");
        let program = |name: &str, answer: &str| {
            let binary = temp.path().join(name);
            script(
                &binary,
                &format!("echo \"$1\" >> \"$0.calls\"\nprintf '%s\\n' '{answer}'"),
            );
            vec![OsString::from(binary), path.clone().into()]
        };
        let same: Launchers = [
            (
                "rust",
                program("rust", r#"{"valid":true,"blobs":32,"bytes":64}"#),
            ),
            ("python", program("python", r#"{"bytes":64,"blobs":32}"#)),
        ];
        let result = saves(&same, &path, &[], 2).expect("identical");
        assert_eq!(result["bytes"], 64);
        assert_eq!(result["identical_results"], true);
        for name in ["rust", "python"] {
            // Three runs each; the warm-up is not among the samples.
            assert_eq!(result["process_wall_ms"][name]["samples"], 2);
            assert_eq!(
                fs::read_to_string(temp.path().join(format!("{name}.calls"))).expect("calls"),
                format!("{}\n", path.display()).repeat(3)
            );
        }
        let different: Launchers = [
            ("rust", program("rust", r#"{"blobs":32}"#)),
            ("python", program("python", r#"{"blobs":31}"#)),
        ];
        assert_eq!(
            saves(&different, &path, &[], 1).expect_err("different").0,
            "Save workload results differ"
        );
        let failing = temp.path().join("failing");
        script(&failing, "echo 'Invalid save state' >&2\nexit 1");
        let broken: Launchers = [
            ("rust", vec![failing.into()]),
            ("python", program("python", "{}")),
        ];
        assert_eq!(
            saves(&broken, &path, &[], 1).expect_err("failing").0,
            "The rust save check failed with exit status 1: Invalid save state"
        );
    }

    #[test]
    fn a_request_is_timed_and_must_succeed() {
        let service = Service::start(|_, path, headers, _| {
            let status = if path == "/api/status" { 200 } else { 500 };
            (
                status,
                json!({"language": headers.get("accept-language"), "token": headers.get("x-flightdeck-token")}),
            )
        });
        let (elapsed, value) =
            request(service.port, "/api/status", Some("session")).expect("answer");
        assert!(elapsed > 0.0);
        assert_eq!(value, json!({"language": ["en"], "token": ["session"]}));
        let (_, anonymous) = request(service.port, "/api/status", None).expect("answer");
        assert_eq!(anonymous["token"], Value::Null);
        assert!(request(service.port, "/api/setup", None).is_err());
    }

    #[test]
    fn python_programs_report_their_output_or_their_error() {
        let temp = tempfile::tempdir().expect("temp");
        let interpreter = temp.path().join("python3");
        script(
            &interpreter,
            "if [ \"$3\" = fail ]; then echo 'No module named flightdeck' >&2; exit 1; fi\necho \" $1 ${#2} $3 \"",
        );
        assert_eq!(
            python(&interpreter, "print(1)", &["source".as_ref()]).expect("output"),
            "-c 8 source"
        );
        let error = python(&interpreter, "print(1)", &["fail".as_ref()]).expect_err("failure");
        assert!(
            error.0.ends_with("failed: No module named flightdeck"),
            "{}",
            error.0
        );
        assert!(python(&temp.path().join("missing"), "", &[]).is_err());
    }
}
