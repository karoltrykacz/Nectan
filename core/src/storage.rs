use crate::{devices::Username, storage_utils::DataWriter};
use anyhow::Result;
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::RwLock,
};

#[derive(Serialize, Deserialize, Clone)]
/// In bytes/s
pub struct SpeedLimits {
    pub upload: u64,
    pub download: u64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct StoredShortcut {
    pub keys: Vec<String>, // e.g. ["Control", "T"];
    pub alt_keys: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub username: Option<Username>,
    pub key: Option<SigningKey>,
    pub theme: String,
    pub registered: bool,
    pub autoaccept_from_nearby: bool,
    pub autoaccept_from_favourites: bool,
    pub default_destination: PathBuf,
    pub language: String,
    pub speed_limits: Option<SpeedLimits>,
    pub shortcuts: HashMap<String, StoredShortcut>,
}

fn default_language() -> String {
    String::from("en")
}

fn default_destination() -> PathBuf {
    dirs::download_dir().expect("Failed to get downloads directory")
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            username: None,
            theme: "light".into(),
            shortcuts: HashMap::default(),
            key: None,
            registered: false,
            autoaccept_from_nearby: false,
            autoaccept_from_favourites: false,
            default_destination: default_destination(),
            language: default_language(),
            speed_limits: None,
        }
    }
}

pub struct Storage {
    inner: RwLock<Settings>,
    writer: DataWriter,
}

impl Storage {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let writer = DataWriter::new(path, "main_storage")
            .expect("Failed to initialize Storage writer.");
        let initial = writer.load::<Settings>().unwrap_or_default();

        Storage {
            inner: RwLock::new(initial),
            writer,
        }
    }

    pub fn read<R>(&self, f: impl FnOnce(&Settings) -> R) -> R {
        f(&self.inner.read().unwrap())
    }

    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> Result<()> {
        let mut guard = self.inner.write().unwrap();
        let mut next = guard.clone();
        f(&mut next);
        self.writer.write(&next)?;
        *guard = next;
        Ok(())
    }

    pub fn theme(&self) -> String {
        self.read(|s| s.theme.clone())
    }

    pub fn registered(&self) -> bool {
        self.read(|s| s.registered)
    }

    pub fn username(&self) -> Option<Username> {
        self.read(|s| s.username.clone())
    }

    pub fn key(&self) -> Option<SigningKey> {
        self.read(|s| s.key.clone())
    }

    pub fn autoaccept_from_nearby(&self) -> bool {
        self.read(|s| s.autoaccept_from_nearby)
    }

    pub fn autoaccept_from_favourites(&self) -> bool {
        self.read(|s| s.autoaccept_from_favourites)
    }

    pub fn default_dir(&self) -> PathBuf {
        self.read(|s| s.default_destination.clone())
    }

    pub fn speed_limits(&self) -> Option<SpeedLimits> {
        self.read(|s| s.speed_limits.clone())
    }

    pub fn dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Nectan")
    }

    pub fn language(&self) -> String {
        self.read(|s| {
            if s.language.is_empty() {
                "en".to_string()
            } else {
                s.language.clone()
            }
        })
    }

    pub fn shortcuts(&self) -> HashMap<String, StoredShortcut> {
        self.read(|s| s.shortcuts.clone())
    }

    pub fn set_shortcut(
        &self,
        id: &str,
        is_primary: bool,
        parts: Vec<String>,
    ) -> Result<()> {
        self.update(|s| {
            let entry = s.shortcuts.entry(id.to_string()).or_default();
            if is_primary {
                entry.keys = parts;
            } else {
                entry.alt_keys = parts;
            }
        })
    }
}

const DEFAULTS: &[(&str, &[&str], &[&str])] = &[
    ("close", &["Escape"], &["Control", "W"]),
    ("confirm", &["Return"], &[]),
    ("move-down", &["J"], &["DownArrow"]),
    ("move-up", &["K"], &["UpArrow"]),
    ("essentials", &["E"], &[]),
    ("toggle-sidebar", &["Control", "B"], &[]),
    ("transfers", &["Control", "T"], &[]),
    ("settings", &["Control", "I"], &[]),
    ("go-parent", &["H"], &["Alt", "UpArrow"]),
    ("go-inside", &["L"], &["Control", "DownArrow"]),
    ("history-forward", &["Control", "I"], &["Alt", "RightArrow"]),
    ("history-back", &["Control", "O"], &["Control", "LeftArrow"]),
    ("half-page-down", &["Control", "D"], &["PageDown"]),
    ("half-page-up", &["Control", "U"], &["PageUp"]),
    ("find", &["Slash"], &["Control", "F"]),
    ("ripgrep", &["Control", "Slash"], &["Control", "Shift", "F"]),
    ("copy", &["Y"], &["Control", "C"]),
    ("cut", &["D"], &["Control", "X"]),
    ("paste", &["P"], &["Control", "V"]),
    ("undo", &["Control", "Z"], &["U"]),
    ("redo", &["Control", "Y"], &["Control", "R"]),
    ("trash", &["Shift", "D"], &["Delete"]),
    ("delete-permanently", &["Control", "Shift", "D"], &[]),
    ("select-all", &["Control", "A"], &[]),
    ("select-regex", &["Control", "S"], &[]),
    ("compress", &["Shift", "Z"], &[]),
    ("rename", &["R"], &["F2"]),
    ("properties", &["Alt", "Return"], &[]),
    ("refresh", &["Control", "L"], &["F5"]),
    ("create", &["Control", "Shift", "N"], &[]),
    ("toggle-hidden", &["Control", "H"], &[]),
    ("list-view", &["Control", "Digit1"], &[]),
    ("grid-view", &["Control", "Digit2"], &[]),
    ("zoom-out", &["Control", "HyphenMinus"], &[]),
    ("zoom-in", &["Control", "Plus"], &[]),
];

pub fn default_shortcut(id: &str) -> StoredShortcut {
    DEFAULTS
        .iter()
        .find(|(i, ..)| *i == id)
        .map(|(_, k, a)| StoredShortcut {
            keys: k.iter().map(|s| s.to_string()).collect(),
            alt_keys: a.iter().map(|s| s.to_string()).collect(),
        })
        .unwrap_or_default()
}
