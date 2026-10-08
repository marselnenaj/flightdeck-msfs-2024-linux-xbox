// SPDX-License-Identifier: MIT
//! Installer results are not equivalent to the setup bootstrapper's exit code.
use crate::{Error, Result, error::require, files, wine::Wine};
use std::{
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::Path,
    time::{Duration, Instant},
};

/// Keep the same compatibility settings when Windows starts a child through
/// Explorer or a shortcut, which may not inherit the Unix launch environment.
/// This registry belongs to this Wine profile, never the Linux host.
pub fn prepare(wine: &Wine<'_>) -> Result<()> {
    for (name, value) in crate::wine::DOTNET_COMPATIBILITY {
        if wine.cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        wine.reg(r"HKCU\Environment", name, value, "REG_SZ")?;
    }
    Ok(())
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Hooks {
    pending: bool,
    failed: bool,
    icu: bool,
    timed_out: bool,
    pub hook: Option<&'static str>,
    pub exit_code: Option<i32>,
    pub package_version: Option<String>,
    pub managed_exception_types: Vec<&'static str>,
    pub clr_exception_code: Option<&'static str>,
    package_is_fenix: bool,
}
impl Hooks {
    pub fn line(&mut self, line: &str) {
        if let Some(id) = line
            .split_once("Package ID:")
            .map(|(_, value)| value.trim())
        {
            self.package_is_fenix = id == "FenixApp";
        }
        if self.package_is_fenix
            && let Some(version) = line
                .split_once("Package Version:")
                .map(|(_, value)| value.trim())
            && crate::fenix_diagnostics::valid_version(version)
        {
            self.package_version = Some(version.into());
        }
        if line.contains("Running --veloapp-install hook")
            || line.contains("Running --veloapp-updated hook")
        {
            self.pending = true;
            self.hook = Some(if line.contains("--veloapp-install") {
                "install"
            } else {
                "updated"
            });
        }
        if line.contains("Hook executed successfully") {
            self.pending = false;
            if !self.failed {
                self.exit_code = Some(0);
            }
        }
        if line.contains("Hook exited with non-zero exit code")
            || line.contains("Hook timed out")
            || line.contains("application install hook failed")
        {
            self.pending = false;
            self.failed = true;
        }
        if line.contains("Hook timed out") {
            self.timed_out = true;
        }
        if let Some(code) = line
            .split_once("Hook exited with non-zero exit code:")
            .and_then(|(_, value)| value.trim().parse::<i32>().ok())
        {
            self.exit_code = Some(code);
        }
        if line.contains("Cannot get symbol") && line.contains("libicu") {
            self.icu = true;
        }
        // Retain only allowlisted symbols from exception headers/chains. An
        // exit status alone never identifies a managed exception or its cause.
        for found in crate::log_reader::regex(
            r"(?:Unhandled [Ee]xception[.:]|Exception [Ii]nfo:|Exception [Tt]ype:|--->)\s*([A-Za-z][A-Za-z0-9_.]*)",
        )
        .captures_iter(line)
        {
            self.exception_type(&found[1]);
        }
        if let Some((kind, _)) = line.trim_start().split_once(':') {
            self.exception_type(kind);
        }
        if crate::log_reader::regex(
            r"(?i)(?:\bunhandled exception(?: code)?\s*[:=]?\s+|\bexception code\s*[:=]\s*)(?:0x)?e0434352\b",
        )
        .is_match(line)
        {
            self.clr_exception_code = Some("e0434352");
        }
    }
    fn exception_type(&mut self, value: &str) {
        if self.managed_exception_types.len()
            < crate::fenix_diagnostics::MAX_MANAGED_EXCEPTION_TYPES
            && let Some(kind) = crate::fenix_diagnostics::managed_exception_type(value)
            && !self.managed_exception_types.contains(&kind)
        {
            self.managed_exception_types.push(kind);
        }
    }
    pub fn has_managed_exception(&self) -> bool {
        !self.managed_exception_types.is_empty() || self.clr_exception_code.is_some()
    }
    pub fn failure(&self) -> Option<&'static str> {
        if self.icu {
            Some("icu_symbol_missing")
        } else if self.timed_out {
            Some("hook_timeout")
        } else if self.has_managed_exception()
            && (self.failed || self.pending || self.exit_code.is_some_and(|code| code != 0))
        {
            Some("managed_exception")
        } else if self.exit_code.is_some_and(|code| code != 0) {
            Some("hook_nonzero")
        } else if self.failed {
            Some("hook_failed")
        } else if self.pending {
            Some("hook_incomplete")
        } else {
            None
        }
    }
    pub fn check(&self) -> Result<()> {
        if self.icu {
            return Err(Error::Invalid(
                "FenixApp konnte die ICU-Sprachbibliothek nicht laden. Die Wine-Einstellungen wurden vorbereitet. Schließe den Installer vollständig und starte die EXE erneut über Flightdeck. Das Windows-Profil muss nicht zurückgesetzt werden.",
            ));
        }
        require(
            !self.failed && !self.pending,
            "Die Fenix-Dateien wurden installiert, aber ein Einrichtungsschritt ist fehlgeschlagen oder wurde nicht abgeschlossen. Schließe den Installer und starte ihn erneut über Flightdeck. Details stehen im lokalen Fenix-Protokoll.",
        )
    }
}

/// Read only this invocation, so an old failed install cannot poison a retry.
/// Read line fragments with a fixed buffer: third-party output is untrusted.
pub fn check_log(path: &Path, original: &File, offset: u64) -> Result<()> {
    scan_log(path, original, offset)?.check()
}
pub fn scan_log(path: &Path, original: &File, offset: u64) -> Result<Hooks> {
    let mut file = files::open_at(rustix::fs::CWD, path, false, false)?;
    let before = original.metadata()?;
    let after = file.metadata()?;
    require(
        (before.dev(), before.ino()) == (after.dev(), after.ino()) && after.len() >= offset,
        "Das Fenix-Protokoll wurde während der Installation verändert.",
    )?;
    file.seek(SeekFrom::Start(offset))?;
    let mut reader = BufReader::with_capacity(8192, file);
    let mut hooks = Hooks::default();
    let mut line = Vec::with_capacity(4096);
    let mut remaining = after.len() - offset;
    // Normal installer logs are kilobytes, not hundreds of megabytes. Refuse
    // to declare success when the result could not be inspected completely.
    require(
        remaining <= 64 * 1024 * 1024,
        "Das Fenix-Protokoll ist zu groß. Bitte den lokalen Installationsfehler prüfen.",
    )?;
    while remaining > 0 {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            break;
        }
        let count = buffer.len().min(remaining as usize);
        for &byte in &buffer[..count] {
            if byte == b'\n' {
                hooks.line(&String::from_utf8_lossy(&line));
                line.clear();
            } else {
                require(
                    line.len() < 4096,
                    "Das Fenix-Protokoll ist zu groß. Bitte den lokalen Installationsfehler prüfen.",
                )?;
                line.push(byte);
            }
        }
        reader.consume(count);
        remaining -= count as u64;
    }
    if !line.is_empty() {
        hooks.line(&String::from_utf8_lossy(&line));
    }
    let completed = reader.get_ref().metadata()?;
    require(
        remaining == 0
            && (
                after.len(),
                after.mtime(),
                after.mtime_nsec(),
                after.ctime(),
                after.ctime_nsec(),
            ) == (
                completed.len(),
                completed.mtime(),
                completed.mtime_nsec(),
                completed.ctime(),
                completed.ctime_nsec(),
            ),
        "Das Fenix-Protokoll wurde während der Installation verändert.",
    )?;
    Ok(hooks)
}

/// Validate the package identity and bounded version before invoking its
/// official post-install command. Only the unique contained FenixApp is used.
pub fn installed_version(prefix: &Path) -> Result<String> {
    let app = crate::fenix_setup::manager(prefix)?;
    let mut executable = files::open_at(rustix::fs::CWD, &app, false, false)?;
    let mut magic = [0; 2];
    std::io::Read::read_exact(&mut executable, &mut magic)?;
    require(magic == *b"MZ", "Ungültige Fenix-Paketmetadaten.")?;
    let directory = app
        .parent()
        .ok_or(Error::Invalid("Ungültige Fenix-Paketmetadaten."))?;
    let package = crate::xml::parse(&files::read(&directory.join("sq.version"), 65536)?)?;
    let metadata = package
        .child("metadata")
        .ok_or(Error::Invalid("Ungültige Fenix-Paketmetadaten."))?;
    let version = metadata
        .child("version")
        .map(|value| value.text())
        .unwrap_or_default();
    require(
        package.named("package")
            && metadata
                .child("id")
                .is_some_and(|value| value.text() == "FenixApp")
            && metadata
                .child("mainExe")
                .is_some_and(|value| value.text() == "FenixApp.exe")
            && crate::fenix_diagnostics::valid_version(&version),
        "Ungültige Fenix-Paketmetadaten.",
    )?;
    Ok(version)
}

/// Run only the official post-install step, never the application/login UI.
/// The caller owns the idle profile reservation; cancellation terminates this
/// subprocess through the same bounded process runner used by setup.
pub fn repair(wine: &Wine<'_>) -> Result<std::process::ExitStatus> {
    if wine.cancel.load(std::sync::atomic::Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    let version = installed_version(&wine.prefix)?;
    let app = crate::fenix_setup::manager(&wine.prefix)?;
    prepare(wine)?;
    if wine.cancel.load(std::sync::atomic::Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    let mut command = wine.command()?;
    command
        .arg(&app)
        .args(["--veloapp-install", &version])
        .current_dir(
            app.parent()
                .ok_or(Error::Invalid("Ungültige Fenix-Paketmetadaten."))?,
        );
    crate::process::run(&mut command, Duration::from_secs(60), wine.cancel)
}

/// Velopack bootstrappers can exit before the installer or app they started.
/// Require a quiet handoff interval; never terminate a successful child here.
#[derive(Default)]
pub struct Handoff {
    quiet_since: Option<Instant>,
}
impl Handoff {
    pub fn complete(
        &mut self,
        now: Instant,
        parent_exited: bool,
        applications_running: bool,
    ) -> bool {
        if !parent_exited || applications_running {
            self.quiet_since = None;
            return false;
        }
        now.duration_since(*self.quiet_since.get_or_insert(now)) >= Duration::from_secs(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_exit_installer_with_failed_hook_is_not_success() {
        let mut hooks = Hooks::default();
        for line in [
            "Running --veloapp-install hook...",
            "Hook exited with non-zero exit code: 3",
            "Installation completed successfully!",
        ] {
            hooks.line(line);
        }
        assert!(hooks.check().is_err());
        // A later, independent hook succeeding does not repair the failed one.
        hooks.line("Hook executed successfully (took 100ms)");
        assert!(hooks.check().is_err());
    }
    #[test]
    fn successful_pending_and_icu_results_are_distinguished() {
        let mut hooks = Hooks::default();
        hooks.line("Running --veloapp-install hook...");
        assert!(hooks.check().is_err());
        hooks.line("Hook executed successfully (took 744ms)");
        assert!(hooks.check().is_ok());
        hooks.line("Unhandled exception. Cannot get symbol u_charsToUChars from libicuuc");
        assert!(
            hooks
                .check()
                .expect_err("ICU failure")
                .to_string()
                .contains("ICU")
        );
    }
    #[test]
    fn handoff_waits_for_detached_installer_and_resets_quiet_interval() {
        let start = Instant::now();
        let mut state = Handoff::default();
        assert!(!state.complete(start, false, false));
        assert!(!state.complete(start, true, false));
        assert!(!state.complete(start + Duration::from_secs(1), true, true));
        assert!(!state.complete(start + Duration::from_secs(5), true, false));
        assert!(state.complete(start + Duration::from_secs(7), true, false));
    }
    #[test]
    fn old_failure_is_ignored_but_current_failure_is_reported() {
        use std::io::Write;
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("installer.log");
        let mut log = crate::process::log(&path).expect("log");
        writeln!(log, "Hook exited with non-zero exit code: 3").expect("write");
        let offset = log.metadata().expect("metadata").len();
        writeln!(
            log,
            "Running --veloapp-install hook...\nHook executed successfully"
        )
        .expect("write");
        assert!(check_log(&path, &log, offset).is_ok());
        writeln!(log, "Hook timed out").expect("write");
        assert!(check_log(&path, &log, offset).is_err());
    }

    #[test]
    fn hook_evidence_contains_only_typed_package_and_failure_fields() {
        let mut hooks = Hooks::default();
        hooks.line("[INFO] Package Version: 9.9.9");
        assert_eq!(hooks.package_version, None);
        hooks.line("[INFO] Package ID: FenixApp");
        hooks.line("[INFO] Package Version: 1.0.286");
        hooks.line("[INFO] Running --veloapp-install hook...");
        hooks.line("[WARN] Hook exited with non-zero exit code: 3");
        assert_eq!(hooks.package_version.as_deref(), Some("1.0.286"));
        assert_eq!(hooks.hook, Some("install"));
        assert_eq!(hooks.exit_code, Some(3));
        assert_eq!(hooks.failure(), Some("hook_nonzero"));
        hooks.line("Hook executed successfully");
        assert_eq!(hooks.failure(), Some("hook_nonzero"));
        hooks.line("Cannot get symbol u_charsToUChars from libicuuc");
        assert_eq!(hooks.failure(), Some("icu_symbol_missing"));
        let mut malicious = Hooks::default();
        malicious.line("Package ID: FenixApp");
        malicious.line("Package Version: user@example.org/token");
        malicious.line("Hook exited with non-zero exit code: https://private.example/token");
        assert_eq!(malicious.package_version, None);
        assert_eq!(malicious.exit_code, None);
    }

    #[test]
    fn managed_exception_evidence_does_not_infer_a_cause_from_exit_82() {
        let mut hooks = Hooks::default();
        hooks.line("[WARN] Hook exited with non-zero exit code: 82");
        assert_eq!(hooks.failure(), Some("hook_nonzero"));
        assert!(!hooks.has_managed_exception());
        for line in [
            "Unhandled exception. System.TypeInitializationException: Initializer for 'PRIVATE_TYPE' failed.",
            " ---> System.IO.FileNotFoundException: Could not load C:\\PRIVATE_PATH\\account@example.test.dll",
            "   at System.IO.FileNotFoundException.ToString()",
            " ---> Private.CustomerException: PRIVATE_MESSAGE",
            " ---> System.IO.FileNotFoundException: duplicated detail",
        ] {
            hooks.line(line);
        }
        assert_eq!(
            hooks.managed_exception_types,
            [
                "System.TypeInitializationException",
                "System.IO.FileNotFoundException"
            ]
        );
        assert_eq!(hooks.clr_exception_code, None);
        assert_eq!(hooks.failure(), Some("managed_exception"));
        let evidence = format!("{hooks:?}");
        for private in [
            "PRIVATE_TYPE",
            "PRIVATE_PATH",
            "PRIVATE_MESSAGE",
            "account@example.test",
            "Private.CustomerException",
        ] {
            assert!(!evidence.contains(private));
        }
    }

    #[test]
    fn official_startup_hook_failure_shape_keeps_types_without_the_private_message() {
        let mut hooks = Hooks::default();
        // Shape observed from official FenixApp 1.0.286 with a deliberately
        // invalid startup hook in an isolated profile. Message data is synthetic.
        for line in [
            "Unhandled exception. System.ArgumentException: The startup hook simple assembly name 'PRIVATE_STARTUP_HOOK' is invalid.",
            " ---> System.IO.FileNotFoundException: Could not load file or assembly 'PRIVATE_STARTUP_HOOK'.",
            "   at System.Reflection.RuntimeAssembly.InternalLoad(AssemblyName assemblyName)",
            "   --- End of inner exception stack trace ---",
        ] {
            hooks.line(line);
        }
        assert_eq!(
            hooks.managed_exception_types,
            [
                "System.ArgumentException",
                "System.IO.FileNotFoundException"
            ]
        );
        assert!(hooks.clr_exception_code.is_none());
        assert_eq!(hooks.failure(), None);
        hooks.line("Hook exited with non-zero exit code: 82");
        assert_eq!(hooks.failure(), Some("managed_exception"));
        assert!(!format!("{hooks:?}").contains("PRIVATE_STARTUP_HOOK"));
    }

    #[test]
    fn clr_code_requires_an_explicit_exception_log_and_canonicalizes_only_that_code() {
        for line in [
            "wine: Unhandled exception 0xe0434352 in thread 0100 at address 000000007B00ABCD (thread 0100), starting debugger...",
            "0100:err:seh:NtRaiseException Unhandled exception code E0434352 flags 1 addr 0x1234",
            "Exception code: 0xE0434352",
        ] {
            let mut hooks = Hooks::default();
            hooks.line("Hook exited with non-zero exit code: 82");
            hooks.line(line);
            assert_eq!(hooks.clr_exception_code, Some("e0434352"));
            assert!(hooks.managed_exception_types.is_empty());
            assert_eq!(hooks.failure(), Some("managed_exception"));
        }
        for line in [
            "Hook exited with non-zero exit code: 82",
            "note: file /private/e0434352/customer.txt",
            "code=e0434352",
            "Exception code: 0xe04343520",
            "Exception code: 0xc0000005",
            "Unhandled exception. Private.CustomerException: account@example.test",
            "Unhandled exception. System.IO.FileNotFoundExceptionPrivate: secret",
            "   at System.IO.FileNotFoundException.ToString()",
        ] {
            let mut hooks = Hooks::default();
            hooks.line("Hook exited with non-zero exit code: 82");
            hooks.line(line);
            assert!(
                !hooks.has_managed_exception(),
                "unexpected evidence from {line}"
            );
            assert_eq!(hooks.failure(), Some("hook_nonzero"));
        }
    }

    #[test]
    fn managed_exception_symbols_are_deduplicated_and_bounded() {
        let mut hooks = Hooks::default();
        for kind in crate::fenix_diagnostics::MANAGED_EXCEPTION_TYPES {
            for _ in 0..3 {
                hooks.line(&format!("Exception Info: {kind}"));
            }
        }
        assert_eq!(hooks.managed_exception_types.len(), 8);
        assert_eq!(
            hooks.managed_exception_types,
            crate::fenix_diagnostics::MANAGED_EXCEPTION_TYPES[..8]
        );
        // Observed symbols are not by themselves a failed-hook result.
        assert_eq!(hooks.failure(), None);
    }

    #[test]
    fn observed_exception_symbols_do_not_change_a_successful_hook_result() {
        let mut hooks = Hooks::default();
        for line in [
            "Running --veloapp-install hook...",
            "Exception Info: System.InvalidOperationException",
            "Exception code: 0xe0434352",
            "Hook executed successfully (took 100ms)",
        ] {
            hooks.line(line);
        }
        assert!(hooks.has_managed_exception());
        assert_eq!(hooks.exit_code, Some(0));
        assert_eq!(hooks.failure(), None);
        assert!(hooks.check().is_ok());
    }

    #[test]
    fn oversized_line_cannot_hide_a_failed_hook() {
        use std::io::Write;
        let temp = tempfile::tempdir().expect("fixture");
        let path = temp.path().join("installer.log");
        let mut log = crate::process::log(&path).expect("log");
        log.write_all(&vec![b'x'; 4096]).expect("long line");
        writeln!(log, "Hook exited with non-zero exit code: 3").expect("failure suffix");
        assert!(scan_log(&path, &log, 0).is_err());
    }

    #[test]
    fn repair_validates_package_then_runs_only_official_hook_with_scoped_settings() {
        use std::{os::unix::fs::PermissionsExt, sync::atomic::AtomicBool};
        const CHILD: &str = "FLIGHTDECK_TEST_FENIX_REPAIR_CHILD";
        if std::env::var_os(CHILD).is_none() {
            // Other tests can fork while this fixture writes its synthetic Wine
            // executable. Their inherited writable FD survives until exec and
            // can cause ETXTBSY even after atomic() closes the original handle.
            // Create and execute the fixture in a single-test child instead.
            let (status, output) = crate::process::output_status(
                std::process::Command::new(std::env::current_exe().expect("test executable"))
                    .args([
                        "--exact",
                        "fenix_installer::tests::repair_validates_package_then_runs_only_official_hook_with_scoped_settings",
                        "--test-threads=1",
                    ])
                    .env(CHILD, "1"),
                Duration::from_secs(20),
                65536,
                &AtomicBool::new(false),
            )
            .expect("isolated repair fixture");
            let output = String::from_utf8_lossy(&output);
            assert!(status.success(), "{output}");
            assert!(output.contains("test result: ok. 1 passed;"), "{output}");
            return;
        }
        let temp = tempfile::tempdir().expect("fixture");
        let prefix = temp.path().join("prefix");
        let current = prefix.join("drive_c/users/steamuser/AppData/Local/FenixApp/current");
        files::private_dir(&current).expect("app directory");
        files::private_dir(&prefix.join("drive_c/windows/system32")).expect("system32");
        files::atomic(&current.join("FenixApp.exe"), b"MZfixture").expect("app");
        let manifest = current.join("sq.version");
        files::atomic(&manifest, b"<package><metadata><id>FenixApp</id><version>1.0.286</version><mainExe>FenixApp.exe</mainExe></metadata></package>").expect("metadata");
        assert_eq!(installed_version(&prefix).expect("version"), "1.0.286");
        let runner = temp.path().join("runner");
        files::private_dir(&runner.join("files/bin")).expect("runner");
        let executable = runner.join("files/bin/wine");
        files::atomic(&executable, b"#!/bin/sh\nprintf '%s\\n' \"$*\"\n[ \"$DOTNET_SYSTEM_GLOBALIZATION_USENLS\" = 1 ] && [ \"$DOTNET_ReadyToRun\" = 0 ]\n").expect("fixture executable");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .expect("mode");
        let log = temp.path().join("repair.log");
        let cancel = AtomicBool::new(false);
        let wine = Wine::new(&prefix, &runner, &log, &cancel).expect("wine");
        assert!(repair(&wine).expect("repair").success());
        let output = String::from_utf8(files::read(&log, 65536).expect("log")).expect("utf8");
        let lines = output.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 3);
        assert!(
            lines[0].starts_with("reg add HKCU\\Environment /v DOTNET_SYSTEM_GLOBALIZATION_USENLS")
        );
        assert!(lines[1].starts_with("reg add HKCU\\Environment /v DOTNET_ReadyToRun"));
        assert!(lines[2].ends_with("FenixApp.exe --veloapp-install 1.0.286"));
        cancel.store(true, std::sync::atomic::Ordering::Release);
        assert!(matches!(repair(&wine), Err(Error::Cancelled)));
        assert_eq!(
            files::read(&log, 65536).expect("no cancelled subprocess"),
            output.as_bytes()
        );
        cancel.store(false, std::sync::atomic::Ordering::Release);
        files::atomic(&manifest, b"<package><metadata><id>OtherApp</id><version>1.0.286</version><mainExe>FenixApp.exe</mainExe></metadata></package>").expect("wrong identity");
        assert!(repair(&wine).is_err());
        assert_eq!(
            files::read(&log, 65536).expect("unchanged log"),
            output.as_bytes()
        );
        files::atomic(&manifest, b"<package><metadata><id>FenixApp</id><version>1.0.286 --unsafe</version><mainExe>FenixApp.exe</mainExe></metadata></package>").expect("bad version");
        assert!(repair(&wine).is_err());
    }
}
