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
        wine.reg(r"HKCU\Environment", name, value, "REG_SZ")?;
    }
    Ok(())
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Hooks {
    pending: bool,
    failed: bool,
    icu: bool,
}
impl Hooks {
    pub fn line(&mut self, line: &str) {
        if line.contains("Running --veloapp-install hook")
            || line.contains("Running --veloapp-updated hook")
        {
            self.pending = true;
        }
        if line.contains("Hook executed successfully") {
            self.pending = false;
        }
        if line.contains("Hook exited with non-zero exit code")
            || line.contains("Hook timed out")
            || line.contains("application install hook failed")
        {
            self.pending = false;
            self.failed = true;
        }
        if line.contains("Cannot get symbol") && line.contains("libicu") {
            self.icu = true;
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
            } else if line.len() < 4096 {
                line.push(byte);
            }
        }
        reader.consume(count);
        remaining -= count as u64;
    }
    if !line.is_empty() {
        hooks.line(&String::from_utf8_lossy(&line));
    }
    hooks.check()
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
}
