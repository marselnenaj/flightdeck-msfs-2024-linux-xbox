use flightdeck_ui::{App, Edition, Language};
use serde_json::json;
pub fn fixture() -> App {
    let mut app = App::new(Edition::Msfs2024);
    app.online = true;
    app.startup_checked = true;
    app.snapshot.insert("status",json!({
        "runtime":{"configured":true,"path":"/synthetic/msfs2024","game_id":"msfs2024","ready":true,"checks":[{"label":"Spielinstallation","detail":"MSFS 2024 und MicrosoftGame.Config vorhanden.","ok":true},{"label":"Kompatibilitäts-Runtime","detail":"Wine-Runner und Xbox-Dienst sind verfügbar.","ok":true},{"label":"Lokaler Spielstandspeicher","detail":"Lokaler Speicher für Cloud-Abgleich und Sicherungen aktiviert.","ok":true}]},
        "game":{"state":"stopped","managed":false,"can_start":true,"can_stop":false},"setup":{"busy":false},
        "cloud":{"state":"idle","enabled":true},"versions":{"msfs2024":{"ready":true},"msfs2020":{"ready":false}},
        "saves":{"available":true,"can_backup":true,"bytes":24576,"files":3,"backups":1},
        "graphics":{"available":true,"nvidia_mode":"auto"},"vr":{"available":true,"mode":"off"}
    }));
    for key in [
        "setup",
        "game-update",
        "launcher-update",
        "cloud-saves",
        "fenix",
        "gsx",
        "proton",
        "maintenance",
        "store-check",
        "mods",
        "diagnostics",
        "problem-reports",
    ] {
        app.snapshot.insert(key, json!({}));
    }
    app.snapshot.insert("setup",json!({"available":true,"install_available":true,"prepare_available":true,"directory_picker":true,"job":null}));
    app.snapshot.insert("proton",json!({"runtime_path":"/synthetic/msfs2024","selected":"Flightdeck (Xodus)","experimental":false,"can_restore":false,"error":"","fenix":false,"job":null}));
    app.discoveries.insert("proton/discover",json!({"choices":[
        {"path":"/synthetic/Steam/Proton - Experimental","label":"Proton - Experimental","version":"experimental-11.0-test","fenix":false},
        {"path":"/synthetic/Steam/proton-cachyos","label":"proton-cachyos","version":"cachyos-10.0-sunset","fenix":true}
    ]}));
    app.snapshot.insert("mods",json!({"state":"ready","runtime_path":"/synthetic/msfs2024","can_open":true,"can_remove":true,"folder_path":"/synthetic/Community","message":"Der Community-Ordner wurde gefunden.","count":0,"mods":[]}));
    app.snapshot.insert("fenix",json!({"state":"installed","runtime_path":"/synthetic/msfs2024","can_change":true,"installed":true,"fenix_installed":true,"settings_ready":true,"manager_installed":true,"configured":true,"can_restore":true}));
    app.snapshot.insert("gsx",json!({"state":"available","runtime_path":"/synthetic/msfs2024","can_change":true,"prepared":true,"package_installed":true,"startup_found":true,"configured":true}));
    app.snapshot.insert("launcher-update",json!({"managed":true,"installed_version":"0.2.2","can_check":true,"can_install":true,"check_id":"checked-launcher","latest_version":"0.2.3","notes":"Synthetic release notes"}));
    app.snapshot.insert("game-update",json!({"available":true,"installed_version":"1.6.0","latest_version":"1.6.0","can_check":true,"can_repair":true,"integrity":{"can_check":true,"available":true}}));
    app.snapshot.insert("cloud-saves",json!({"available":true,"can_check":true,"can_download":true,"can_prepare_import":true,"can_import":true,"can_upload":true,"can_restore":true,"plan":{"id":"reviewed-plan","container_count":3,"local_container_count":2,"blob_count":6,"total_bytes":24576,"add_count":1,"replace_count":2,"delete_count":0,"unchanged_count":0,"conflict_count":1},"restore_id":"owned-backup"}));
    app
}

pub fn localized_fixture(language: Language) -> App {
    let mut app = fixture();
    app.language = language;
    if language == Language::En {
        app.snapshot.get_mut("mods").expect("mods")["message"] =
            json!("The Community folder was found.");
        app.snapshot.get_mut("status").expect("status")["runtime"]["checks"] = json!([
            {"label":"Game installation", "detail":"MSFS 2024 and MicrosoftGame.Config are present.", "ok":true},
            {"label":"Compatibility runtime", "detail":"Wine runner and Xbox service are available.", "ok":true},
            {"label":"Local save storage", "detail":"Local storage for cloud sync and backups is enabled.", "ok":true}
        ]);
    }
    app
}
