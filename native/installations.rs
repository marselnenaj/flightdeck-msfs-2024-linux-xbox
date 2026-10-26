// SPDX-License-Identifier: MIT
//! Installation identities are independent of game edition and storefront.
//! Steam discovery is read-only, including when a prefix has not been created.
use crate::{
    Error, Result,
    backend::{Launcher, string},
    error::require,
    files,
    games::Game,
    runtime,
};
use fenix_patch::vdf::{self, Block, Item};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
};

const FILE: &str = "installations.json";
const INVALID: &str = "Die Installationsauswahl ist ungültig. Bitte Installationen erneut suchen.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Client {
    Native,
    Flatpak,
    Snap,
}
impl Client {
    pub fn name(self) -> &'static str {
        match self {
            Self::Native => "Steam",
            Self::Flatpak => "Steam (Flatpak)",
            Self::Snap => "Steam (Snap)",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Xbox {
        path: PathBuf,
    },
    Steam {
        client: Client,
        root: PathBuf,
        library: PathBuf,
        install_dir: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installation {
    pub id: String,
    pub game: Game,
    pub source: Source,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SteamRoot {
    pub client: Client,
    pub path: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Registry {
    pub schema: u32,
    pub selected: Option<String>,
    pub entries: Vec<Installation>,
    #[serde(default)]
    pub steam_roots: Vec<SteamRoot>,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            schema: 1,
            selected: None,
            entries: Vec::new(),
            steam_roots: Vec::new(),
        }
    }
}
fn clean(path: &Path) -> bool {
    path.is_absolute()
        && path.components().all(|c| {
            matches!(
                c,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
}
impl Installation {
    pub fn new(game: Game, source: Source) -> Result<Self> {
        let id = files::sha256(&serde_json::to_vec(&json!({"game":game,"source":source}))?)[..32]
            .to_string();
        Ok(Self { id, game, source })
    }
    pub fn appid(&self) -> &'static str {
        match self.game {
            Game::Msfs2020 => "1250410",
            Game::Msfs2024 => "2537590",
        }
    }
    pub fn steam(&self) -> bool {
        matches!(self.source, Source::Steam { .. })
    }
    pub fn target(&self) -> Result<fenix_patch::steam::Target> {
        match &self.source {
            Source::Steam { root, library, .. } => {
                fenix_patch::steam::Target::new(self.game.id(), root, library)
                    .map_err(|_| Error::Invalid(INVALID))
            }
            _ => Err(Error::Invalid(INVALID)),
        }
    }
    pub fn game_path(&self) -> PathBuf {
        match &self.source {
            Source::Xbox { path } => self.game.path(path),
            Source::Steam {
                library,
                install_dir,
                ..
            } => library.join("steamapps/common").join(install_dir),
        }
    }
    pub fn prefix(&self) -> PathBuf {
        match &self.source {
            Source::Xbox { path } => path.join("local/msfs-prefix"),
            Source::Steam { library, .. } => library
                .join("steamapps/compatdata")
                .join(self.appid())
                .join("pfx"),
        }
    }
    pub fn label(&self) -> String {
        format!(
            "{} · {}",
            self.game.name(),
            match self.source {
                Source::Xbox { .. } => "Xbox",
                Source::Steam { client, .. } => client.name(),
            }
        )
    }
    /// Evidence belongs to the profile directory, independently of manifest
    /// timestamps or where a library has been moved on the same filesystem.
    pub fn prefix_generation(&self) -> String {
        self.prefix()
            .symlink_metadata()
            .ok()
            .filter(|m| m.is_dir())
            .map(|m| {
                files::sha256(&serde_json::to_vec(&json!([m.dev(), m.ino()])).unwrap_or_default())
                    [..24]
                    .to_string()
            })
            .unwrap_or_else(|| "uninitialized".into())
    }
    /// Bind actions to both the installation and the currently observed layout.
    pub fn context(&self) -> String {
        if let Source::Xbox { path } = &self.source {
            return path.to_string_lossy().into_owned();
        }
        let stamp = |p: &Path| {
            p.symlink_metadata()
                .ok()
                .map(|m| json!([m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec()]))
        };
        let manifest = match &self.source {
            Source::Steam { library, .. } => library
                .join("steamapps")
                .join(format!("appmanifest_{}.acf", self.appid())),
            _ => PathBuf::new(),
        };
        let bytes = serde_json::to_vec(&json!({"source":self.source,"manifest":stamp(&manifest),"prefix":self.prefix().symlink_metadata().ok().map(|m|json!([m.dev(),m.ino()]))})).unwrap_or_default();
        format!("steam:{}:{}", self.id, &files::sha256(&bytes)[..24])
    }
    pub fn validate(&self) -> Result<()> {
        require(
            self.id.len() == 32 && self.id.bytes().all(|b| b.is_ascii_hexdigit()),
            INVALID,
        )?;
        match &self.source {
            Source::Xbox { path } => {
                runtime::validate(&path.to_string_lossy())?;
                require(Game::for_runtime(path)? == self.game, INVALID)
            }
            Source::Steam {
                client,
                root,
                library,
                install_dir,
            } => {
                require(
                    clean(root)
                        && clean(library)
                        && root.canonicalize()? == *root
                        && library.canonicalize()? == *library,
                    INVALID,
                )?;
                files::directory(root, false)?;
                files::directory(library, false)?;
                client_binding(*client, root, &files::home())?;
                require(single_name(install_dir), INVALID)?;
                let registered = libraries(root)?;
                require(
                    registered.contains(library),
                    "Die Steam-Bibliothek ist nicht mehr registriert. Bitte erneut suchen.",
                )?;
                // -applaunch chooses an AppID within a client, not a library.
                // Never prepare one prefix while Steam could launch another.
                for other in registered.iter().filter(|other| *other != library) {
                    require(
                        manifest(self.game, other)?.is_none(),
                        "Diese Steam-App ist in mehreren Bibliotheken registriert. Bereinige zuerst die veraltete Installation in Steam und suche dann erneut.",
                    )?;
                }
                let found = manifest(self.game, library)?;
                require(
                    found.as_deref() == Some(install_dir),
                    "Die Steam-Installation wurde verschoben oder geändert. Bitte erneut suchen.",
                )
            }
        }
    }
    pub fn public(&self, selected: bool) -> Value {
        let accessible = self.validate().is_ok();
        let initialized = self.prefix().join("system.reg").is_file();
        json!({"id":self.id,"game_id":self.game.id(),"name":self.label(),"source":if self.steam(){"steam"}else{"xbox"},"path":self.game_path(),"prefix":self.prefix(),"context":self.context(),"selected":selected,"available":accessible,"prefix_ready":initialized,"needs_first_launch":self.steam()&&!initialized,"beta":self.steam(),"steam_client":match self.source{Source::Steam{client,..}=>Some(client),_=>None}})
    }
}
fn client_binding(client: Client, root: &Path, home: &Path) -> Result<()> {
    let aliases: &[&str] = match client {
        Client::Native => return Ok(()),
        Client::Flatpak => &[
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
            ".var/app/com.valvesoftware.Steam/data/.local/share/Steam",
        ],
        Client::Snap => &["snap/steam/common/.local/share/Steam"],
    };
    require(
        aliases
            .iter()
            .any(|p| home.join(p).canonicalize().is_ok_and(|p| p == root)),
        "Dieser Steam-Ordner gehört nicht zum gewählten Flatpak- oder Snap-Client. Wähle den passenden Client und seinen Datenordner.",
    )
}
fn single_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.chars().any(char::is_control)
        && Path::new(name).components().count() == 1
        && Path::new(name).file_name().is_some()
        && !name.contains(['/', '\\'])
}
fn entry<'a>(block: &'a Block, key: &str) -> Result<Option<&'a Item>> {
    let mut items = block
        .entries
        .iter()
        .filter(|e| e.key.eq_ignore_ascii_case(key));
    let first = items.next();
    require(items.next().is_none(), INVALID)?;
    Ok(first.map(|e| &e.value))
}
fn parse(path: &Path) -> Result<Block> {
    let raw = files::read_public(path, 2 * 1024 * 1024)?;
    let text = std::str::from_utf8(&raw).map_err(|_| Error::Invalid(INVALID))?;
    vdf::parse(text).map_err(|_| Error::Invalid(INVALID))
}
fn text<'a>(block: &'a Block, key: &str) -> Result<Option<&'a str>> {
    Ok(match entry(block, key)? {
        Some(Item::Text(v)) => Some(v),
        None => None,
        _ => return Err(Error::Invalid(INVALID)),
    })
}
pub fn libraries(root: &Path) -> Result<BTreeSet<PathBuf>> {
    let mut result = BTreeSet::from([root.to_path_buf()]);
    let path = root.join("steamapps/libraryfolders.vdf");
    if !files::exists(&path) {
        return Ok(result);
    }
    let doc = parse(&path)?;
    let Some(Item::Block(block)) = entry(&doc, "libraryfolders")? else {
        return Err(Error::Invalid(INVALID));
    };
    require(block.entries.len() <= 256, INVALID)?;
    for item in &block.entries {
        if !item.key.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let value = match &item.value {
            Item::Text(v) => Some(v.as_str()),
            Item::Block(v) => text(v, "path")?,
        };
        if let Some(value) = value {
            let path = Path::new(value);
            if clean(path)
                && let Ok(path) = path.canonicalize()
                && path.join("steamapps").is_dir()
            {
                result.insert(path);
            }
        }
    }
    Ok(result)
}
fn manifest(game: Game, library: &Path) -> Result<Option<String>> {
    let appid = match game {
        Game::Msfs2020 => "1250410",
        Game::Msfs2024 => "2537590",
    };
    let path = library
        .join("steamapps")
        .join(format!("appmanifest_{appid}.acf"));
    if !files::exists(&path) {
        return Ok(None);
    }
    let doc = parse(&path)?;
    let Some(Item::Block(app)) = entry(&doc, "AppState")? else {
        return Err(Error::Invalid(INVALID));
    };
    require(text(app, "appid")? == Some(appid), INVALID)?;
    let name = text(app, "installdir")?.ok_or(Error::Invalid(INVALID))?;
    require(single_name(name), INVALID)?;
    let common = library.join("steamapps/common");
    let folder = common.join(name);
    if !folder.is_dir() {
        return Ok(None);
    }
    require(
        folder.canonicalize()?.starts_with(common.canonicalize()?),
        INVALID,
    )?;
    Ok(Some(name.to_string()))
}
pub fn roots(home: &Path, extra: &[SteamRoot]) -> Vec<SteamRoot> {
    let mut candidates = vec![];
    for (client, name) in [
        (Client::Native, ".local/share/Steam"),
        (Client::Native, ".steam/root"),
        (Client::Native, ".steam/steam"),
        (
            Client::Flatpak,
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        ),
        (
            Client::Flatpak,
            ".var/app/com.valvesoftware.Steam/data/.local/share/Steam",
        ),
        (Client::Snap, "snap/steam/common/.local/share/Steam"),
    ] {
        candidates.push(SteamRoot {
            client,
            path: home.join(name),
        });
    }
    candidates.extend_from_slice(extra);
    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter_map(|mut r| {
            r.path = r.path.canonicalize().ok()?;
            if r.path.join("steamapps").is_dir() && seen.insert(r.path.clone()) {
                Some(r)
            } else {
                None
            }
        })
        .take(32)
        .collect()
}
pub fn discover(
    home: &Path,
    registry: &Registry,
    known: &BTreeMap<String, PathBuf>,
    selected: Option<&Path>,
) -> Vec<Installation> {
    let mut result = Vec::new();
    for (game, path) in known
        .iter()
        .filter_map(|(id, p)| Game::select(id).ok().map(|g| (g, p)))
    {
        if let Ok(path) = runtime::validate(&path.to_string_lossy())
            && let Ok(item) = Installation::new(game, Source::Xbox { path })
        {
            result.push(item);
        }
    }
    if let Some(path) = selected
        && let Ok(game) = Game::for_runtime(path)
        && let Ok(path) = runtime::validate(&path.to_string_lossy())
        && !result
            .iter()
            .any(|i| matches!(&i.source,Source::Xbox{path:p} if *p==path))
        && let Ok(item) = Installation::new(game, Source::Xbox { path })
    {
        result.push(item);
    }
    for root in roots(home, &registry.steam_roots) {
        let Ok(libraries) = libraries(&root.path) else {
            continue;
        };
        for library in libraries {
            for game in Game::ALL {
                if let Ok(Some(install_dir)) = manifest(game, &library)
                    && let Ok(mut item) = Installation::new(
                        game,
                        Source::Steam {
                            client: root.client,
                            root: root.path.clone(),
                            library: library.clone(),
                            install_dir,
                        },
                    )
                {
                    if let Some(old) = registry.entries.iter().find(|old| {
                        old.game == game
                            && match (&old.source, &item.source) {
                                (
                                    Source::Steam {
                                        root: r,
                                        library: l,
                                        ..
                                    },
                                    Source::Steam {
                                        root: r2,
                                        library: l2,
                                        ..
                                    },
                                ) => {
                                    r == r2
                                        && (l == l2
                                            || !l
                                                .join("steamapps")
                                                .join(format!("appmanifest_{}.acf", old.appid()))
                                                .exists())
                                }
                                _ => false,
                            }
                    }) {
                        item.id = old.id.clone();
                    }
                    result.push(item);
                }
            }
        }
    }
    for old in &registry.entries {
        if !result.iter().any(|i| i.id == old.id) {
            result.push(old.clone());
        }
    }
    result.truncate(128);
    result
}
fn validate_registry(registry: &Registry) -> Result<()> {
    require(
        registry.schema == 1 && registry.entries.len() <= 128 && registry.steam_roots.len() <= 16,
        INVALID,
    )?;
    let mut seen = BTreeSet::new();
    for item in &registry.entries {
        require(
            item.id.len() == 32
                && item.id.bytes().all(|b| b.is_ascii_hexdigit())
                && seen.insert(&item.id),
            INVALID,
        )?;
        match &item.source {
            Source::Xbox { path } => require(clean(path), INVALID)?,
            Source::Steam {
                root,
                library,
                install_dir,
                ..
            } => require(
                clean(root) && clean(library) && single_name(install_dir),
                INVALID,
            )?,
        }
    }
    require(
        registry
            .selected
            .as_ref()
            .is_none_or(|id| registry.entries.iter().any(|i| &i.id == id)),
        INVALID,
    )?;
    for root in &registry.steam_roots {
        require(clean(&root.path), INVALID)?;
    }
    Ok(())
}
pub fn recover(state: &Path) -> Result<()> {
    let journal = state.join("installation-selection.json");
    if !files::exists(&journal) {
        return Ok(());
    }
    let value: Value = files::json(&journal, 2 * 1024 * 1024)?;
    require(value["schema"] == 1 && value["legacy"].is_object(), INVALID)?;
    let registry: Registry = serde_json::from_value(value["registry"].clone())?;
    validate_registry(&registry)?;
    files::atomic_json(&state.join("config.json"), &value["legacy"])?;
    files::atomic_json(&state.join(FILE), &registry)?;
    std::fs::remove_file(journal)?;
    files::directory(state, true)?.sync_all()?;
    Ok(())
}
pub fn load(state: &Path) -> Result<Registry> {
    let path = state.join(FILE);
    if !files::exists(&path) {
        return Ok(Registry::default());
    }
    let registry: Registry = files::json(&path, 1024 * 1024)?;
    validate_registry(&registry)?;
    Ok(registry)
}
pub fn save(state: &Path, registry: &Registry) -> Result<()> {
    validate_registry(registry)?;
    files::atomic_json(&state.join(FILE), registry)
}
pub fn commit_selection(state: &Path, registry: &Registry, legacy: &Value) -> Result<()> {
    validate_registry(registry)?;
    require(legacy.is_object(), INVALID)?;
    files::atomic_json(
        &state.join("installation-selection.json"),
        &json!({"schema":1,"registry":registry,"legacy":legacy}),
    )?;
    recover(state)
}
pub fn snapshot(app: &Launcher) -> Value {
    let s = app.lock();
    let items = discover(
        &files::home(),
        &s.installations,
        &s.known,
        s.runtime.as_deref(),
    );
    let selected = s.steam.as_ref().map(|i| i.id.clone()).or_else(|| {
        items
            .iter()
            .find(|i| matches!(&i.source,Source::Xbox{path} if Some(path)==s.runtime.as_ref()))
            .map(|i| i.id.clone())
    });
    json!({"installations":items.iter().map(|i|i.public(selected.as_deref()==Some(&i.id))).collect::<Vec<_>>(),"selected_id":selected,"can_select":s.active.is_none()&&s.process.is_none()&&!s.closing&&!Launcher::external(&s)})
}
pub fn select(app: &Arc<Launcher>, data: &Value) -> Result<Value> {
    let id = string(data, "installation_id")?;
    let mut s = app.lock();
    Launcher::idle(&mut s)?;
    let items = discover(
        &files::home(),
        &s.installations,
        &s.known,
        s.runtime.as_deref(),
    );
    let item = items
        .into_iter()
        .find(|i| i.id == id)
        .ok_or(Error::Invalid(INVALID))?;
    require(
        data["binding"] == item.context(),
        "Die Installation hat sich geändert. Bitte erneut suchen.",
    )?;
    item.validate()?;
    let mut registry = s.installations.clone();
    registry.entries.retain(|i| i.id != id);
    registry.entries.push(item.clone());
    registry.selected = Some(id.to_string());
    match &item.source {
        Source::Steam { .. } => {
            save(&app.state_dir, &registry)?;
            s.installations = registry;
            s.steam = Some(item);
            s.steam_session = None;
            s.steam_helpers = None;
            s.steam_notice.clear();
            s.exit_code = None;
        }
        Source::Xbox { path } => {
            let path = path.clone();
            drop(s);
            app.configure(&path.to_string_lossy())?;
        }
    }
    Ok(json!({"ok":true}))
}
pub fn add_root(app: &Arc<Launcher>, data: &Value) -> Result<Value> {
    let path = files::expand(string(data, "steam_root")?).canonicalize()?;
    files::directory(&path, false)?;
    require(
        path.join("steamapps").is_dir() && path.join("config/config.vdf").is_file(),
        "Bitte den Steam-Clientordner mit steamapps und config/config.vdf auswählen, nicht nur eine Spielebibliothek.",
    )?;
    libraries(&path)?;
    let client: Client =
        serde_json::from_value(data["client"].clone()).map_err(|_| Error::Invalid(INVALID))?;
    client_binding(client, &path, &files::home())?;
    let mut s = app.lock();
    Launcher::idle(&mut s)?;
    let mut registry = s.installations.clone();
    registry.steam_roots.retain(|r| r.path != path);
    require(registry.steam_roots.len() < 16, INVALID)?;
    registry.steam_roots.push(SteamRoot { client, path });
    save(&app.state_dir, &registry)?;
    s.installations = registry;
    Ok(json!({"ok":true}))
}

pub fn xbox_setup(app: &Arc<Launcher>) -> Result<Value> {
    let mut s = app.lock();
    Launcher::idle(&mut s)?;
    let mut registry = s.installations.clone();
    registry.selected = registry
        .entries
        .iter()
        .find(|i| matches!(&i.source,Source::Xbox{path} if Some(path)==s.runtime.as_ref()))
        .map(|i| i.id.clone());
    save(&app.state_dir, &registry)?;
    s.installations = registry;
    s.steam = None;
    s.steam_session = None;
    s.steam_helpers = None;
    Ok(json!({"ok":true}))
}
