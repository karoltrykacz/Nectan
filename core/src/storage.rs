use crate::{devices::Username, storage_utils::DataWriter};
use anyhow::Result;
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::RwLock,
};

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
            key: None,
            registered: false,
            autoaccept_from_nearby: false,
            autoaccept_from_favourites: false,
            default_destination: default_destination(),
            language: default_language(),
        }
    }
}

pub struct Storage {
    inner: RwLock<Settings>,
    writer: DataWriter,
}

impl Storage {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let writer =
            DataWriter::new(path, "main_storage").expect("Failed to initialize Storage writer.");
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
}
