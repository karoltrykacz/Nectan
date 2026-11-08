use crate::devices::Username;
use anyhow::Result;
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::RwLock,
};
use utils::storage_utils::{DataWriter, PostcardSerializer};

#[derive(Serialize, Deserialize, Clone)]
/// In bytes/s
pub struct SpeedLimits {
    pub upload: u64,
    pub download: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Notifications {
    pub connection: bool,
    pub transfer: bool,
    pub device: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub username: Option<Username>,
    pub key: Option<SigningKey>,
    pub autoaccept_from_nearby: bool,
    pub autoaccept_from_favourites: bool,
    pub default_destination: PathBuf,
    pub notifications: Notifications,
    pub speed_limits: Option<SpeedLimits>,
}

fn default_destination() -> PathBuf {
    dirs::download_dir().unwrap_or(std::env::current_dir().unwrap())
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            username: None,
            key: None,
            autoaccept_from_nearby: false,
            notifications: Notifications {
                connection: true,
                transfer: true,
                device: true,
            },
            autoaccept_from_favourites: false,
            default_destination: default_destination(),
            speed_limits: None,
        }
    }
}

pub struct Storage {
    inner: RwLock<Settings>,
    writer: DataWriter<PostcardSerializer>,
}

impl Storage {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let writer = DataWriter::<PostcardSerializer>::new(
            path,
            "sender_storage",
        )
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

    pub fn update(
        &self,
        f: impl FnOnce(&mut Settings),
    ) -> Result<()> {
        let mut guard = self.inner.write().unwrap();
        let mut next = guard.clone();
        f(&mut next);
        self.writer.write(&next)?;
        *guard = next;
        Ok(())
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
}
