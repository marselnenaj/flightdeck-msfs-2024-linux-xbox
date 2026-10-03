// SPDX-License-Identifier: MIT
use crate::{Error, Result, files};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Game {
    #[serde(rename = "msfs2024")]
    Msfs2024,
    #[serde(rename = "msfs2020")]
    Msfs2020,
}
impl Game {
    pub const ALL: [Self; 2] = [Self::Msfs2024, Self::Msfs2020];
    pub fn select(id: &str) -> Result<Self> {
        match id {
            "msfs2024" => Ok(Self::Msfs2024),
            "msfs2020" => Ok(Self::Msfs2020),
            _ => Err(Error::Invalid("Unbekannte MSFS-Version.")),
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Msfs2024 => "msfs2024",
            Self::Msfs2020 => "msfs2020",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Msfs2024 => "Microsoft Flight Simulator 2024",
            Self::Msfs2020 => "Microsoft Flight Simulator 2020",
        }
    }
    pub fn store_id(self) -> &'static str {
        match self {
            Self::Msfs2024 => "9P38D19T7LRV",
            Self::Msfs2020 => "9NRRJLLXM68V",
        }
    }
    pub fn directory(self) -> &'static str {
        match self {
            Self::Msfs2024 => "MSFS2024",
            Self::Msfs2020 => "MSFS2020",
        }
    }
    pub fn executable(self) -> &'static str {
        match self {
            Self::Msfs2024 => "FlightSimulator2024.exe",
            Self::Msfs2020 => "FlightSimulator.exe",
        }
    }
    pub fn user_config(self) -> &'static str {
        match self {
            Self::Msfs2024 => "Microsoft Flight Simulator 2024",
            Self::Msfs2020 => "Microsoft Flight Simulator",
        }
    }
    pub fn path(self, runtime: &Path) -> PathBuf {
        runtime.join("games").join(self.directory())
    }
    pub fn for_runtime(runtime: &Path) -> Result<Self> {
        let path = runtime.join("private/runtime.json");
        let value = match files::json::<serde_json::Map<String, serde_json::Value>>(&path, 65536) {
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::Msfs2024);
            }
            other => other?,
        };
        match value.get("game_id") {
            None => Ok(Self::Msfs2024),
            Some(value) => Self::select(
                value
                    .as_str()
                    .ok_or(Error::Invalid("Ungültige Runtime-Konfiguration."))?,
            ),
        }
    }
}
