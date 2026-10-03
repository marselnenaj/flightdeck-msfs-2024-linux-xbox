// SPDX-License-Identifier: MIT
//! Bounded Store package metadata shared by setup, updates and mod discovery.
use crate::{Error, Result, error::require, files, games::Game};
use quick_xml::{Reader, XmlVersion, events::Event};
use serde::{Deserialize, Serialize};
use std::path::Path;
const INVALID: &str = "Die heruntergeladene Spielkonfiguration ist ungültig.";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
    pub publisher: String,
    pub version: String,
}
pub struct Config {
    pub identity: Option<Identity>,
    pub store_id: String,
    pub executables: Vec<String>,
}
pub fn text(bytes: &[u8]) -> Result<String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        require(bytes.len().is_multiple_of(2), INVALID)?;
        let little = bytes[0] == 0xff;
        let words: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                if little {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16(&words).map_err(|_| Error::Invalid(INVALID))
    } else {
        String::from_utf8(
            bytes
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(bytes)
                .to_vec(),
        )
        .map_err(|_| Error::Invalid(INVALID))
    }
}
pub fn version(value: &str) -> Result<[u16; 4]> {
    let parts: Vec<_> = value.split('.').collect();
    require(
        parts.len() == 4
            && parts
                .iter()
                .all(|v| !v.is_empty() && v.len() <= 5 && v.bytes().all(|b| b.is_ascii_digit())),
        "Die Spielversion ist nicht eindeutig lesbar.",
    )?;
    let mut out = [0; 4];
    for (i, p) in parts.into_iter().enumerate() {
        out[i] = p
            .parse()
            .map_err(|_| Error::Invalid("Die Spielversion ist nicht eindeutig lesbar."))?;
    }
    Ok(out)
}
pub fn parse(bytes: &[u8]) -> Result<Config> {
    require(bytes.len() <= 1024 * 1024, INVALID)?;
    let text = text(bytes)?;
    let upper = text.to_ascii_uppercase();
    require(
        !upper.contains("<!DOCTYPE") && !upper.contains("<!ENTITY"),
        INVALID,
    )?;
    let mut reader = Reader::from_str(&text);
    let mut depth = 0;
    let mut root = false;
    let mut identity = None;
    let mut identities = 0;
    let mut stores = Vec::new();
    let mut store_depth = None;
    let mut executables = Vec::new();
    loop {
        let event = reader.read_event().map_err(|_| Error::Invalid(INVALID))?;
        let is_empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let local = e.local_name();
                let name = local.as_ref();
                if depth == 0 {
                    require(!root && name == "Game", INVALID)?;
                    root = true;
                }
                if name == "Identity" {
                    identities += 1;
                    let mut attrs = std::collections::BTreeMap::new();
                    for a in e.attributes() {
                        let a = a.map_err(|_| Error::Invalid(INVALID))?;
                        attrs.insert(
                            a.key.as_ref().to_string(),
                            a.normalized_value(XmlVersion::Implicit1_0)
                                .map_err(|_| Error::Invalid(INVALID))?
                                .into_owned(),
                        );
                    }
                    identity = Some(Identity {
                        name: attrs.remove("Name").unwrap_or_default(),
                        publisher: attrs.remove("Publisher").unwrap_or_default(),
                        version: attrs.remove("Version").unwrap_or_default(),
                    });
                }
                if name == "Executable" {
                    for a in e.attributes() {
                        let a = a.map_err(|_| Error::Invalid(INVALID))?;
                        if a.key.as_ref() == "Name" {
                            executables.push(
                                a.normalized_value(XmlVersion::Implicit1_0)
                                    .map_err(|_| Error::Invalid(INVALID))?
                                    .into_owned(),
                            );
                        }
                    }
                }
                if name == "StoreId" {
                    stores.push(String::new());
                    if !is_empty {
                        store_depth = Some(depth + 1);
                    }
                }
                if !is_empty {
                    depth += 1;
                }
            }
            Event::Text(e) => {
                if store_depth == Some(depth) {
                    let value = e.xml10_content();
                    stores.last_mut().ok_or(Error::Invalid(INVALID))?.push_str(
                        &quick_xml::escape::unescape(&value)
                            .map_err(|_| Error::Invalid(INVALID))?,
                    );
                }
            }
            Event::End(_) => {
                require(depth > 0, INVALID)?;
                if store_depth == Some(depth) {
                    store_depth = None;
                }
                depth -= 1;
            }
            Event::DocType(_) | Event::GeneralRef(_) => return Err(Error::Invalid(INVALID)),
            Event::Eof => break,
            _ => {}
        }
    }
    require(
        root && depth == 0 && stores.len() == 1 && identities <= 1,
        INVALID,
    )?;
    Ok(Config {
        identity,
        store_id: stores.remove(0),
        executables,
    })
}
pub fn config(root: &Path) -> Result<Config> {
    let mut file = root.join("MicrosoftGame.Config");
    if !files::exists(&file) {
        file = root.join("MicrosoftGame.config");
    }
    parse(&files::read(&file, 1024 * 1024)?)
}
pub fn validate_download(root: &Path, game: Game) -> Result<()> {
    for name in [".xodus-streaming.msixvc", game.executable()] {
        let file = files::open_at(rustix::fs::CWD, root.join(name), false, false)?;
        require(
            file.metadata()?.len() > 0,
            "Der Spieldownload wurde nicht vollständig abgeschlossen.",
        )?;
    }
    require(
        !files::exists(&root.join(".xodus-streaming-tmp.msixvc")),
        "Der Spieldownload enthält noch unvollständige Daten. Bitte erneut versuchen.",
    )?;
    let config = config(root)?;
    require(
        config.store_id == game.store_id(),
        "Die heruntergeladene Spielkonfiguration gehört nicht zur gewählten MSFS-Version.",
    )?;
    require(
        config
            .executables
            .iter()
            .any(|s| s.eq_ignore_ascii_case(game.executable())),
        INVALID,
    )
}
pub fn installed(root: &Path, game: Game) -> Result<Identity> {
    let config = config(root)?;
    require(config.store_id == game.store_id(), INVALID)?;
    let identity=config.identity.ok_or(Error::Invalid("Für diese Installation fehlen eindeutige MSFS-Store-Identität und Spielversion. Ein Update wird nicht geraten."))?;
    require(
        !identity.name.is_empty() && !identity.publisher.is_empty(),
        INVALID,
    )?;
    version(&identity.version)?;
    Ok(identity)
}
