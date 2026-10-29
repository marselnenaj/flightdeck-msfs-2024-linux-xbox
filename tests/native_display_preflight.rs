// SPDX-License-Identifier: MIT
//! No test invokes hyprctl, Wine, XOpenDisplay or the live desktop.
#![allow(clippy::unwrap_used)]
use flightdeck::{
    Error,
    display_preflight::{self as display, Adapter, Compositor, Desktop},
    files,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const ORIGINAL: &str = "-- private fixture only\ndofile((os.getenv(\"OMARCHY_PATH\") or \"/usr/share/omarchy\") .. \"/default/hypr/bootstrap.lua\")\nrequire(\"default.hypr.omarchy\")\nhl.config({ general = { gaps_in = 6 } })";
const JOURNAL: &str = "display-preflight-repair.json";
const BACKUP: &str = "display-preflight-backup.lua";
const TOKEN: &str = "0123456789abcdef0123456789abcdef";
fn owned_block() -> String {
    format!(
        "\n-- flightdeck-x11-repair:{TOKEN}:begin\nhl.config({{ xwayland = {{ enabled = false }} }})\n-- flightdeck-x11-repair:{TOKEN}:end\n"
    )
}
struct Fixture {
    _temporary: tempfile::TempDir,
    home: PathBuf,
    prefix: PathBuf,
    runner: PathBuf,
    state: PathBuf,
    config: PathBuf,
    env: BTreeMap<String, String>,
}
impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path().join("home");
        let prefix = temporary.path().join("prefix");
        let runner = temporary.path().join("runner");
        let state = temporary.path().join("state");
        let config = home.join(".config/hypr/hyprland.lua");
        for dir in [config.parent().unwrap(), &prefix, &state, &home.join("run")] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(&config, ORIGINAL).unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o640)).unwrap();
        let driver = runner.join("files/lib64/wine/x86_64-unix/winex11.so");
        fs::create_dir_all(driver.parent().unwrap()).unwrap();
        fs::write(driver, b"synthetic driver presence").unwrap();
        Self {
            _temporary: temporary,
            home,
            prefix,
            runner,
            state,
            config,
            env: BTreeMap::from([("DISPLAY".into(), ":0".into())]),
        }
    }
    fn fake(&self) -> Fake {
        Fake {
            desktop: Desktop {
                home: self.home.clone(),
                config_home: self.home.join(".config"),
                runtime_dir: self.home.join("run"),
                signature: "fixture_instance".into(),
            },
            config: self.config.clone(),
            probes: VecDeque::from([
                json!({"schema":1,"state":"unavailable"}),
                json!({"schema":1,"state":"ready"}),
            ]),
            calls: Vec::new(),
            probe_count: 0,
            version: "0.56.2".into(),
            commit: "efb50993780079460b0cbed1363e2166a2de1d9f".into(),
            trusted: true,
            x11_client: false,
            live_xwayland: false,
            config_errors: false,
            disabled_errors: false,
            enabled: true,
            fail_first_reload: false,
            reload_count: 0,
            concurrent_edit: None,
            alter_marker: false,
            cancel_after_disable: None,
            config_override: false,
            owned_display: true,
            probe_error: None,
        }
    }
    fn ensure(&self, fake: &mut Fake) -> flightdeck::Result<Value> {
        display::ensure_with(
            &self.prefix,
            &self.runner,
            &self.env,
            &self.state,
            &AtomicBool::new(false),
            fake,
        )
    }
    fn assert_restored(&self) {
        assert_eq!(fs::read(&self.config).unwrap(), ORIGINAL.as_bytes());
        assert_eq!(fs::metadata(&self.config).unwrap().mode() & 0o777, 0o640);
        assert!(!self.state.join(JOURNAL).exists());
        assert!(!self.state.join(BACKUP).exists());
    }
    fn interrupted(&self, phase: &str, config: &str) {
        fs::write(&self.config, config).unwrap();
        files::atomic(&self.state.join(BACKUP), ORIGINAL.as_bytes()).unwrap();
        files::atomic_json(&self.state.join(JOURNAL), &json!({
            "schema":1,"token":TOKEN,"home":self.home,"original_sha256":files::sha256(ORIGINAL.as_bytes()),
            "original_mode":0o640,"phase":phase,
        })).unwrap();
    }
}
struct Fake {
    desktop: Desktop,
    config: PathBuf,
    probes: VecDeque<Value>,
    calls: Vec<Vec<String>>,
    probe_count: usize,
    version: String,
    commit: String,
    trusted: bool,
    x11_client: bool,
    live_xwayland: bool,
    config_errors: bool,
    disabled_errors: bool,
    enabled: bool,
    fail_first_reload: bool,
    reload_count: usize,
    concurrent_edit: Option<String>,
    alter_marker: bool,
    cancel_after_disable: Option<Arc<AtomicBool>>,
    config_override: bool,
    owned_display: bool,
    probe_error: Option<&'static str>,
}
impl Adapter for Fake {
    fn probe(&mut self, _: &BTreeMap<String, String>, _: &AtomicBool) -> flightdeck::Result<Value> {
        self.probe_count += 1;
        if let Some(message) = self.probe_error.take() {
            return Err(Error::Invalid(message));
        }
        Ok(self
            .probes
            .pop_front()
            .unwrap_or(json!({"schema":1,"state":"ready"})))
    }
    fn desktop(&self) -> flightdeck::Result<Desktop> {
        Ok(self.desktop.clone())
    }
    fn trusted_hyprctl(&self) -> bool {
        self.trusted
    }
    fn compositor(&self, _: u32) -> flightdeck::Result<Compositor> {
        Ok(Compositor {
            uid: files::uid(),
            trusted_executable: true,
            arguments: if self.config_override {
                vec!["/usr/bin/Hyprland".into(), "--config=/other.lua".into()]
            } else {
                vec!["/usr/bin/Hyprland".into()]
            },
            environment: BTreeMap::from([(
                "HOME".into(),
                self.desktop.home.to_string_lossy().into_owned(),
            )]),
            x11_displays: if self.owned_display {
                vec![0]
            } else {
                Vec::new()
            },
        })
    }
    fn xwayland_present(&self) -> flightdeck::Result<bool> {
        Ok(self.live_xwayland)
    }
    fn hyprctl(&mut self, args: &[&str], _: &AtomicBool) -> flightdeck::Result<Vec<u8>> {
        self.calls.push(args.iter().map(|v| (*v).into()).collect());
        let value = match args {
            ["-j", "version"] => json!({"version":self.version,"commit":self.commit,"dirty":false}),
            ["-j", "instances"] => json!([{"instance":self.desktop.signature,"pid":123}]),
            ["-j", "clients"] => json!([{"xwayland":self.x11_client}]),
            ["-j", "getoption", "xwayland:enabled"] => json!({"int":u8::from(self.enabled)}),
            ["configerrors"] => {
                let temporary = fs::read_to_string(&self.config)
                    .unwrap()
                    .contains("-- flightdeck-x11-repair:");
                return Ok(
                    if self.config_errors || (self.disabled_errors && temporary) {
                        b"fixture config error\n".to_vec()
                    } else {
                        Vec::new()
                    },
                );
            }
            ["reload", "config-only"] => {
                self.reload_count += 1;
                let text = fs::read_to_string(&self.config).unwrap();
                self.enabled = !text.contains("-- flightdeck-x11-repair:");
                if !self.enabled {
                    if let Some(edit) = self.concurrent_edit.take() {
                        // An editor's atomic save changes the inode as well as
                        // the bytes. Cleanup must retain this later content.
                        let path = self.config.with_extension("new");
                        fs::write(&path, format!("{text}{edit}")).unwrap();
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
                        fs::rename(path, &self.config).unwrap();
                    }
                    if self.alter_marker {
                        fs::write(
                            &self.config,
                            text.replace("enabled = false", "enabled = true"),
                        )
                        .unwrap();
                    }
                    if let Some(cancel) = &self.cancel_after_disable {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
                if self.fail_first_reload && self.reload_count == 1 {
                    return Err(Error::Invalid("fixture command failure"));
                }
                return Ok(b"ok\n".to_vec());
            }
            _ => panic!("unexpected fake command {args:?}"),
        };
        Ok(serde_json::to_vec(&value).unwrap())
    }
}

#[test]
fn supported_recovery_restores_exact_bytes_mode_and_checks_a_new_handshake() {
    let f = Fixture::new();
    let mut fake = f.fake();
    assert_eq!(
        f.ensure(&mut fake).unwrap(),
        json!({"schema":1,"state":"ready","repaired":true})
    );
    f.assert_restored();
    assert_eq!(fake.probe_count, 2);
    assert_eq!(fake.reload_count, 2);
    assert!(fake.calls.iter().all(|args| {
        !args
            .iter()
            .any(|arg| ["dispatch", "exit", "kill", "exec"].contains(&arg.as_str()))
    }));
}

#[test]
fn healthy_x11_does_not_inspect_or_mutate_the_desktop() {
    let f = Fixture::new();
    let mut fake = f.fake();
    fake.probes = VecDeque::from([
        json!({"schema":1,"state":"ready","untrusted_extra":"must not be echoed"}),
    ]);
    let result = f.ensure(&mut fake).unwrap();
    assert_eq!(result, json!({"schema":1,"state":"ready","repaired":false}));
    assert!(fake.calls.is_empty());
    f.assert_restored();
}

#[test]
fn unsupported_compositor_build_tool_or_config_override_never_changes_configuration() {
    for case in 0..4 {
        let f = Fixture::new();
        let mut fake = f.fake();
        match case {
            0 => fake.version = "0.57.0".into(),
            1 => fake.commit = "other".into(),
            2 => fake.trusted = false,
            _ => fake.config_override = true,
        }
        let error = f.ensure(&mut fake).unwrap_err();
        assert!(matches!(error, Error::Graphics(_)));
        assert_eq!(
            display::failure_code(&error),
            Some("display_recovery_unsupported")
        );
        assert_eq!(fake.reload_count, 0);
        f.assert_restored();
    }
}

#[test]
fn existing_x11_client_or_xwayland_process_prevents_the_toggle() {
    for client in [false, true] {
        let f = Fixture::new();
        let mut fake = f.fake();
        fake.x11_client = client;
        fake.live_xwayland = !client;
        assert_eq!(
            display::failure_code(&f.ensure(&mut fake).unwrap_err()),
            Some("display_recovery_busy")
        );
        assert_eq!(fake.reload_count, 0);
        f.assert_restored();
    }
}

#[test]
fn symlink_hardlink_and_writable_configuration_are_refused() {
    for case in 0..3 {
        let f = Fixture::new();
        let mut fake = f.fake();
        let other = f.home.join("original.lua");
        match case {
            0 => {
                fs::rename(&f.config, &other).unwrap();
                symlink(&other, &f.config).unwrap();
            }
            1 => {
                fs::hard_link(&f.config, &other).unwrap();
            }
            _ => fs::set_permissions(&f.config, fs::Permissions::from_mode(0o666)).unwrap(),
        }
        assert_eq!(
            display::failure_code(&f.ensure(&mut fake).unwrap_err()),
            Some("display_config_changed")
        );
        assert_eq!(fake.reload_count, 0);
        assert!(!f.state.join(JOURNAL).exists());
    }
}

#[test]
fn concurrent_atomic_editor_save_is_merged_by_removing_only_owned_marker() {
    let f = Fixture::new();
    let mut fake = f.fake();
    let edit = "\n-- a later user edit\nhl.config({ general = { gaps_out = 8 } })\n";
    fake.concurrent_edit = Some(edit.into());
    f.ensure(&mut fake).unwrap();
    assert_eq!(
        fs::read_to_string(&f.config).unwrap(),
        format!("{ORIGINAL}{edit}")
    );
    assert_eq!(fs::metadata(&f.config).unwrap().mode() & 0o777, 0o640);
    assert!(!f.state.join(JOURNAL).exists());
}

#[test]
fn cancellation_after_disable_finishes_restoration_before_returning() {
    let f = Fixture::new();
    let mut fake = f.fake();
    let cancel = Arc::new(AtomicBool::new(false));
    fake.cancel_after_disable = Some(cancel.clone());
    let error = display::ensure_with(&f.prefix, &f.runner, &f.env, &f.state, &cancel, &mut fake)
        .unwrap_err();
    assert!(matches!(error, Error::Cancelled));
    f.assert_restored();
    assert_eq!(fake.reload_count, 2);
}

#[test]
fn failed_disable_reload_or_configuration_errors_always_restore() {
    for config_error in [false, true] {
        let f = Fixture::new();
        let mut fake = f.fake();
        fake.disabled_errors = config_error;
        fake.fail_first_reload = !config_error;
        assert!(f.ensure(&mut fake).is_err());
        f.assert_restored();
        assert_eq!(fake.reload_count, 2);
    }
}

#[test]
fn initial_configuration_errors_do_not_create_a_backup_or_reload() {
    let f = Fixture::new();
    let mut fake = f.fake();
    fake.config_errors = true;
    assert_eq!(
        display::failure_code(&f.ensure(&mut fake).unwrap_err()),
        Some("display_config_errors")
    );
    assert_eq!(fake.reload_count, 0);
    f.assert_restored();
}

#[test]
fn modified_owned_block_is_never_guessed_away_and_retains_private_recovery_files() {
    let f = Fixture::new();
    let mut fake = f.fake();
    fake.alter_marker = true;
    assert_eq!(
        display::failure_code(&f.ensure(&mut fake).unwrap_err()),
        Some("display_restore_pending")
    );
    assert!(f.state.join(JOURNAL).exists());
    assert_eq!(fs::read(f.state.join(BACKUP)).unwrap(), ORIGINAL.as_bytes());
    assert_eq!(
        fs::metadata(f.state.join(BACKUP)).unwrap().mode() & 0o777,
        0o600
    );
    assert!(
        fs::read_to_string(&f.config)
            .unwrap()
            .contains("enabled = true")
    );
}

#[test]
fn startup_journal_replay_preserves_later_edits_and_is_idempotent() {
    let f = Fixture::new();
    let later = "\n-- written after the process crashed\n";
    f.interrupted("installed", &format!("{ORIGINAL}{}{later}", owned_block()));
    let mut fake = f.fake();
    fake.enabled = false;
    assert!(display::restore_pending_with(&f.state, &mut fake).unwrap());
    assert_eq!(
        fs::read_to_string(&f.config).unwrap(),
        format!("{ORIGINAL}{later}")
    );
    assert!(!display::restore_pending_with(&f.state, &mut fake).unwrap());
    assert_eq!(fake.reload_count, 1);
    assert_eq!(fake.probe_count, 0);
}

#[test]
fn prepared_journal_without_mutation_is_cleared_without_touching_desktop() {
    let f = Fixture::new();
    f.interrupted("prepared", ORIGINAL);
    let mut fake = f.fake();
    assert!(!display::restore_pending_with(&f.state, &mut fake).unwrap());
    f.assert_restored();
    assert!(fake.calls.is_empty());
}

#[test]
fn corrupt_backup_keeps_pending_journal_and_config_unchanged() {
    let f = Fixture::new();
    let temporary = format!("{ORIGINAL}{}", owned_block());
    f.interrupted("installed", &temporary);
    fs::write(f.state.join(BACKUP), b"corrupt").unwrap();
    let mut fake = f.fake();
    assert_eq!(
        display::failure_code(&display::restore_pending_with(&f.state, &mut fake).unwrap_err()),
        Some("display_restore_pending")
    );
    assert_eq!(fs::read_to_string(&f.config).unwrap(), temporary);
    assert_eq!(fake.reload_count, 0);
    assert!(f.state.join(JOURNAL).exists());
}

#[test]
fn native_wayland_environment_registry_appdefaults_and_minimal_runners_skip_probe() {
    for case in 0..5 {
        let mut f = Fixture::new();
        let mut fake = f.fake();
        match case {
            0 => { f.env.insert("PROTON_ENABLE_WAYLAND".into(), "1".into()); },
            1 => fs::write(f.prefix.join("user.reg"), "[Software\\\\Wine\\\\Drivers]\n\"Graphics\"=\"wayland,x11\"\n").unwrap(),
            2 => fs::write(f.prefix.join("user.reg"), "[Software\\\\Wine\\\\AppDefaults\\\\FlightSimulator2024.exe\\\\Drivers]\n\"Graphics\"=\"wayland\"\n").unwrap(),
            3 => { f.env.insert("WINEDLLOVERRIDES".into(), "winex11.drv=d;other=n".into()); },
            _ => fs::remove_file(f.runner.join("files/lib64/wine/x86_64-unix/winex11.so")).unwrap(),
        }
        assert_eq!(f.ensure(&mut fake).unwrap()["state"], "not_required");
        assert_eq!(fake.probe_count, 0);
        assert!(fake.calls.is_empty());
        f.assert_restored();
    }
}

#[test]
fn missing_library_is_a_graphics_failure_without_attempting_desktop_repair() {
    let f = Fixture::new();
    let mut fake = f.fake();
    fake.probes = VecDeque::from([json!({"schema":1,"state":"missing_library"})]);
    let error = f.ensure(&mut fake).unwrap_err();
    assert!(matches!(error, Error::Graphics(_)));
    assert_eq!(
        display::failure_code(&error),
        Some("display_missing_library")
    );
    assert!(fake.calls.is_empty());
    assert!(!error.to_string().contains(":private-fixture"));
    f.assert_restored();
}

#[test]
fn pending_repair_is_replayed_before_a_new_native_wayland_profile_skips() {
    let mut f = Fixture::new();
    f.interrupted("installed", &format!("{ORIGINAL}{}", owned_block()));
    f.env.insert("PROTON_ENABLE_WAYLAND".into(), "1".into());
    let mut fake = f.fake();
    assert_eq!(f.ensure(&mut fake).unwrap()["state"], "not_required");
    assert_eq!(fake.probe_count, 0);
    assert_eq!(fake.reload_count, 1);
    f.assert_restored();
}

#[test]
fn unrelated_remote_or_unowned_local_display_does_not_toggle_compositor() {
    for case in 0..3 {
        let mut f = Fixture::new();
        let mut fake = f.fake();
        match case {
            0 => {
                f.env.insert("DISPLAY".into(), "remote.example:0".into());
            }
            1 => {
                f.env.remove("DISPLAY");
            }
            _ => fake.owned_display = false,
        }
        assert!(f.ensure(&mut fake).is_err());
        assert_eq!(fake.reload_count, 0);
        f.assert_restored();
    }
}

#[test]
fn helper_protocol_or_spawn_error_is_not_evidence_to_mutate_desktop() {
    let f = Fixture::new();
    let mut fake = f.fake();
    fake.probe_error = Some("fixture helper unavailable");
    assert_eq!(
        display::failure_code(&f.ensure(&mut fake).unwrap_err()),
        Some("display_probe_failed")
    );
    assert!(fake.calls.is_empty());
    f.assert_restored();
}

#[test]
fn actual_bounded_handshake_timeout_permits_the_same_narrow_recovery() {
    let f = Fixture::new();
    let mut fake = f.fake();
    fake.probe_error = Some("Der Vorgang hat nicht rechtzeitig geantwortet.");
    fake.probes = VecDeque::from([json!({"schema":1,"state":"ready"})]);
    f.ensure(&mut fake).unwrap();
    assert_eq!(fake.reload_count, 2);
    f.assert_restored();
}

#[test]
fn completed_cleanup_replays_even_when_backup_was_removed_before_a_crash() {
    let f = Fixture::new();
    f.interrupted("completed", ORIGINAL);
    fs::remove_file(f.state.join(BACKUP)).unwrap();
    let mut fake = f.fake();
    assert!(!display::restore_pending_with(&f.state, &mut fake).unwrap());
    assert!(fake.calls.is_empty());
    f.assert_restored();
}

#[test]
fn prepared_without_backup_is_known_to_precede_any_config_mutation() {
    let f = Fixture::new();
    f.interrupted("prepared", ORIGINAL);
    fs::remove_file(f.state.join(BACKUP)).unwrap();
    let mut fake = f.fake();
    assert!(!display::restore_pending_with(&f.state, &mut fake).unwrap());
    assert!(fake.calls.is_empty());
    f.assert_restored();
}

#[test]
fn pending_cleanup_precedes_registry_read_errors() {
    let f = Fixture::new();
    f.interrupted("installed", &format!("{ORIGINAL}{}", owned_block()));
    let outside = f.home.join("outside.reg");
    fs::write(&outside, b"not a prefix registry").unwrap();
    symlink(outside, f.prefix.join("user.reg")).unwrap();
    let mut fake = f.fake();
    assert!(f.ensure(&mut fake).is_err());
    f.assert_restored();
    assert_eq!(fake.reload_count, 1);
}

#[test]
fn undo_removes_own_disk_override_after_an_upgrade_but_keeps_live_reload_pending() {
    let f = Fixture::new();
    f.interrupted("installed", &format!("{ORIGINAL}{}", owned_block()));
    let mut fake = f.fake();
    fake.version = "0.57.0".into();
    assert_eq!(
        display::failure_code(&display::restore_pending_with(&f.state, &mut fake).unwrap_err()),
        Some("display_restore_pending")
    );
    assert_eq!(fs::read_to_string(&f.config).unwrap(), ORIGINAL);
    assert!(f.state.join(JOURNAL).exists());
    assert!(f.state.join(BACKUP).exists());
    assert_eq!(fake.reload_count, 0);
}
