// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{
    Error, Result, fenix_setup, files, framework,
    framework_repair::{self, SetupWine},
    wine_processes, xml,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
fn write(path: &Path, bytes: &[u8]) {
    files::private_dir(path.parent().unwrap()).unwrap();
    files::atomic(path, bytes).unwrap();
}
fn framework_files(prefix: &Path, x86: bool, x64: bool, release: u32) {
    let mut reg = String::new();
    for (present, middle, arch) in [
        (x86, r"Wow6432Node\\", "Framework"),
        (x64, "", "Framework64"),
    ] {
        reg.push_str(&format!("[Software\\\\{middle}Microsoft\\\\NET Framework Setup\\\\NDP\\\\v4\\\\Full] 123\n\"Release\"=dword:{release:08x}\n"));
        for name in ["clr.dll", "csc.exe"] {
            let path = prefix.join(format!(
                "drive_c/windows/Microsoft.NET/{arch}/v4.0.30319/{name}"
            ));
            if present {
                write(&path, b"MZfixture");
            } else if path.exists() {
                fs::remove_file(path).unwrap();
            }
        }
    }
    write(&prefix.join("system.reg"), reg.as_bytes());
}
struct Fake {
    prefix: PathBuf,
    calls: Vec<Vec<String>>,
    attempts: usize,
    succeed: usize,
    listing: String,
    broken_probes: usize,
    unreachable_server: bool,
}
impl Fake {
    fn new(prefix: &Path) -> Self {
        Self {
            prefix: prefix.into(),
            calls: Vec::new(),
            attempts: 0,
            succeed: 1,
            listing: String::new(),
            broken_probes: 0,
            unreachable_server: false,
        }
    }
}
impl SetupWine for Fake {
    fn prefix(&self) -> &Path {
        &self.prefix
    }
    fn run(&mut self, args: &[String], _installer: bool, _timeout: Duration) -> Result<()> {
        self.calls.push(args.to_vec());
        if self.unreachable_server && args[0] == "wineboot" {
            return Err(Error::Invalid("fixture unreachable wineserver"));
        }
        if args[0].ends_with("NDP48-x86-x64-AllOS-ENU.exe")
            && !args.iter().any(|v| v == "/uninstall")
        {
            self.attempts += 1;
            if self.attempts >= self.succeed {
                framework_files(&self.prefix, true, true, 528049);
            }
        }
        if args[0].ends_with("csc.exe") && self.broken_probes > 0 {
            self.broken_probes -= 1;
            return Err(Error::Invalid("fixture CLR startup failure"));
        }
        Ok(())
    }
    fn reg(&mut self, key: &str, name: &str, value: &str, kind: &str) -> Result<()> {
        if name == "mscoree" {
            assert_eq!(
                value, "native,builtin",
                "CLR setup must retain the builtin fallback"
            );
        }
        self.calls
            .push(["reg", key, name, value, kind].map(str::to_string).to_vec());
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        self.calls.push(vec!["stop".into()]);
        self.unreachable_server = false;
        Ok(())
    }
    fn list(&mut self) -> Result<String> {
        Ok(self.listing.clone())
    }
}
fn prepare(fake: &mut Fake) -> Result<()> {
    framework_repair::prepare(fake, |name, _, _| Ok(PathBuf::from(name)), |_| {})
}
#[test]
fn healthy_framework_executes_both_compilers_without_download() {
    let temp = tempfile::tempdir().unwrap();
    framework_files(temp.path(), true, true, 528049);
    let mut fake = Fake::new(temp.path());
    framework_repair::prepare(
        &mut fake,
        |_, _, _| panic!("healthy CLR must not download"),
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fake.calls
            .iter()
            .filter(|v| v[0].ends_with("csc.exe"))
            .count(),
        2
    );
    assert_eq!(fake.attempts, 0);
}
#[test]
fn failed_clr_connection_is_reset_before_reboot_without_reinstalling_dotnet() {
    let temp = tempfile::tempdir().unwrap();
    framework_files(temp.path(), true, true, 528049);
    let mut fake = Fake::new(temp.path());
    fake.broken_probes = 1;
    fake.unreachable_server = true;
    framework_repair::prepare(
        &mut fake,
        |_, _, _| panic!("healthy CLR must not download"),
        |_| {},
    )
    .unwrap();
    let stop = fake.calls.iter().position(|v| v[0] == "stop").unwrap();
    let reboot = fake.calls.iter().position(|v| v[0] == "wineboot").unwrap();
    assert!(stop < reboot);
    assert_eq!(fake.attempts, 0);
    assert_eq!(
        fake.calls
            .iter()
            .filter(|v| v[0].ends_with("csc.exe"))
            .count(),
        3
    );
}
#[test]
fn missing_architecture_triggers_repair_and_restores_win10() {
    let temp = tempfile::tempdir().unwrap();
    framework_files(temp.path(), false, true, 528049);
    let mut fake = Fake::new(temp.path());
    prepare(&mut fake).unwrap();
    assert!(
        fake.calls
            .iter()
            .any(|v| v[0].ends_with("NDP48-x86-x64-AllOS-ENU.exe")
                && v.iter().any(|v| v == "/repair"))
    );
    assert!(framework::status(temp.path()).ready);
    assert!(fake.calls.iter().any(|v| v.iter().any(|v| v == "win10")));
}
#[test]
fn mono_registry_advertisement_without_clrs_still_bootstraps_40() {
    let temp = tempfile::tempdir().unwrap();
    framework_files(temp.path(), false, false, 533320);
    let mut fake = Fake::new(temp.path());
    fake.listing = "{11111111-2222-4333-8444-555555555555}|Wine Mono Runtime\n".into();
    prepare(&mut fake).unwrap();
    assert!(
        fake.calls
            .iter()
            .any(|v| v[0].ends_with("dotNetFx40_Full_x86_x64.exe"))
    );
    assert!(fake.calls.iter().any(|v| v[0] == "uninstaller"));
    assert!(framework::status(temp.path()).ready);
}

#[test]
fn steam_keeps_its_windows7_bootstrap_policy_and_xbox_keeps_windows_xp() {
    for first in ["win7", "winxp"] {
        let temp = tempfile::tempdir().unwrap();
        framework_files(temp.path(), false, false, 0);
        let mut fake = Fake::new(temp.path());
        if first == "win7" {
            framework_repair::prepare_with_version(
                &mut fake,
                |name, _, _| Ok(PathBuf::from(name)),
                |_| {},
                first,
            )
            .unwrap();
        } else {
            prepare(&mut fake).unwrap();
        }
        let install = fake
            .calls
            .iter()
            .position(|args| args[0].ends_with("dotNetFx40_Full_x86_x64.exe"))
            .unwrap();
        let version = fake.calls[..install]
            .iter()
            .rev()
            .find(|args| args[0] == "reg" && args[2] == "Version")
            .unwrap();
        assert_eq!(version[3], first);
        assert_eq!(
            fake.calls
                .iter()
                .filter(|args| args[0].ends_with("csc.exe"))
                .count(),
            2
        );
        assert!(
            fake.calls
                .iter()
                .any(|args| args[0] == "reg" && args[2] == "Version" && args[3] == "win10")
        );
    }
}
#[test]
fn no_op_msi_repair_falls_back_to_a_bounded_reinstall() {
    let temp = tempfile::tempdir().unwrap();
    framework_files(temp.path(), false, true, 528049);
    let mut fake = Fake::new(temp.path());
    fake.succeed = 2;
    prepare(&mut fake).unwrap();
    assert_eq!(fake.attempts, 2);
    assert!(
        fake.calls
            .iter()
            .any(|v| v.iter().any(|v| v == "/uninstall"))
    );
    assert!(
        fake.calls
            .iter()
            .any(|v| v[0].ends_with("dotNetFx40_Full_x86_x64.exe"))
    );
    assert!(framework::status(temp.path()).ready);
}
#[test]
fn native_newer_framework_is_never_downgraded() {
    let temp = tempfile::tempdir().unwrap();
    framework_files(temp.path(), false, true, 533320);
    let mut fake = Fake::new(temp.path());
    assert!(
        framework_repair::prepare(
            &mut fake,
            |_, _, _| panic!("must not obtain a downgrade"),
            |_| {}
        )
        .is_err()
    );
    assert_eq!(fake.attempts, 0);
}
#[test]
fn success_registry_without_working_clr_is_repaired() {
    let temp = tempfile::tempdir().unwrap();
    framework_files(temp.path(), true, true, 528049);
    let mut fake = Fake::new(temp.path());
    fake.broken_probes = 2;
    prepare(&mut fake).unwrap();
    assert_eq!(fake.attempts, 1);
}
#[test]
fn exhausted_repair_is_bounded_and_stops_staging_wine() {
    let temp = tempfile::tempdir().unwrap();
    framework_files(temp.path(), false, true, 528049);
    let mut fake = Fake::new(temp.path());
    fake.succeed = usize::MAX;
    assert!(prepare(&mut fake).is_err());
    assert_eq!(fake.attempts, 2);
    assert_eq!(fake.calls.last().unwrap(), &["stop"]);
    assert!(fake.calls.iter().any(|v| v.iter().any(|v| v == "win10")));
}
#[test]
fn fenix_settings_preserve_other_addons_and_remove_only_duplicate_fenix_entries() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(&root.join(fenix_setup::PROGRAM).join("Fenix.exe"), b"MZ");
    write(
        &root.join(fenix_setup::CONFIG).join("fenixConfig.xml"),
        b"<Settings><displayMode>GPU</displayMode><custom>keep &amp; this</custom></Settings>",
    );
    write(
        &root.join(fenix_setup::CONFIG).join("persistancy.xml"),
        b"<Settings><fcuReadoutsType>1</fcuReadoutsType></Settings>",
    );
    let exe =
        root.join("drive_c/users/pilot/AppData/Roaming/Microsoft Flight Simulator 2024/exe.xml");
    write(&exe,b"<SimBase.Document><Launch.Addon><Name>Keep Me</Name><Path>Other.exe</Path></Launch.Addon><Launch.Addon><Name>Fenix Old</Name></Launch.Addon><Launch.Addon><Name>Fenix Duplicate</Name></Launch.Addon></SimBase.Document>");
    assert_eq!(
        fenix_setup::configure_prefix(root, fenix_setup::DisplayRenderer::Cpu).unwrap(),
        (true, true)
    );
    assert_eq!(
        fenix_setup::configure_prefix(root, fenix_setup::DisplayRenderer::Cpu).unwrap(),
        (true, true)
    );
    let parsed = xml::parse(&fs::read(&exe).unwrap()).unwrap();
    assert_eq!(
        parsed
            .items
            .iter()
            .filter(|v| matches!(v,xml::Item::Element(v) if v.named("Launch.Addon")))
            .count(),
        2
    );
    let text = fs::read_to_string(exe).unwrap();
    assert!(text.contains("Keep Me"));
    assert!(text.contains("FenixBootstrapper.exe"));
    assert!(
        fs::read_to_string(root.join(fenix_setup::CONFIG).join("fenixConfig.xml"))
            .unwrap()
            .contains("keep &amp; this")
    );
}
#[test]
fn fenix_displays_and_autostart_do_not_wait_for_the_readout_settings() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let config = root.join(fenix_setup::CONFIG);
    let exe =
        root.join("drive_c/users/pilot/AppData/Roaming/Microsoft Flight Simulator 2024/exe.xml");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    assert_eq!(
        fenix_setup::configure_prefix(root, fenix_setup::DisplayRenderer::Cpu).unwrap(),
        (false, false)
    );
    write(&root.join(fenix_setup::PROGRAM).join("Fenix.exe"), b"MZ");
    // Without the sign-in settings nothing counts as configured.
    write(&config.join("persistancy.xml"), b"<Settings/>");
    assert_eq!(
        fenix_setup::configure_prefix(root, fenix_setup::DisplayRenderer::Cpu).unwrap(),
        (false, false)
    );
    fs::remove_file(config.join("persistancy.xml")).unwrap();
    // Fenix can create persistancy.xml later; it is not invented here.
    write(&config.join("fenixConfig.xml"), b"<Settings/>");
    assert_eq!(
        fenix_setup::configure_prefix(root, fenix_setup::DisplayRenderer::Cpu).unwrap(),
        (true, false)
    );
    assert!(!config.join("persistancy.xml").exists());
    assert!(
        fs::read_to_string(config.join("fenixConfig.xml"))
            .unwrap()
            .contains("<displayMode>CPU</displayMode>")
    );
    assert!(
        fs::read_to_string(&exe)
            .unwrap()
            .contains("FenixBootstrapper.exe")
    );
    write(&config.join("persistancy.xml"), b"<Settings/>");
    assert_eq!(
        fenix_setup::configure_prefix(root, fenix_setup::DisplayRenderer::Cpu).unwrap(),
        (true, true)
    );
    assert!(
        fs::read_to_string(config.join("persistancy.xml"))
            .unwrap()
            .contains("<fcuReadoutsType>0</fcuReadoutsType>")
    );
}
#[test]
fn unsafe_xml_and_foreign_webviews_are_rejected_or_left_alone() {
    for text in [
        "<!DOCTYPE R [<!ENTITY external SYSTEM 'file:///etc/passwd'>]><R/>",
        "<R></Other>",
        "<R/><Second/>",
    ] {
        assert!(xml::parse(text.as_bytes()).is_err());
    }
    let args = |v: &[&str]| v.iter().map(|v| v.to_string()).collect::<Vec<_>>();
    assert_eq!(
        wine_processes::name(&args(&[
            r"C:\Edge\msedgewebview2.exe",
            "--webview-exe-name=FenixApp.exe"
        ])),
        "fenix-webview2"
    );
    assert_eq!(
        wine_processes::name(&args(&[
            "msedgewebview2.exe",
            "--user-data-dir",
            r"C:\ProgramData\Fenix\App\WebView2\EBWebView"
        ])),
        "fenix-webview2"
    );
    assert_eq!(
        wine_processes::name(&args(&[
            "msedgewebview2.exe",
            "--user-data-dir",
            r"C:\AnotherAddon\WebView"
        ])),
        "msedgewebview2.exe"
    );
}
