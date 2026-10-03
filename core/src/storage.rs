use std::{
    collections::HashMap,
    hash::Hash,
    path::PathBuf,
    sync::{Arc, RwLock},
};

use crate::{
    devices::Username,
    storage_utils::{DataWriter, KvStore},
};
use anyhow::{Result, anyhow};
use base64::engine::Config;
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Username,
    Theme,
    Key,
    Registered,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Value {
    Username(Username),
    Theme(Theme),
    Key(SigningKey),
    Registered(bool),
}

impl Value {
    pub fn key(&self) -> Key {
        match self {
            Value::Username(_) => Key::Username,
            Value::Theme(_) => Key::Theme,
            Value::Key(_) => Key::Key,
            Value::Registered(_) => Key::Registered,
        }
    }
}

pub struct Storage {
    inner: RwLock<HashMap<Key, Value>>,
    writer: DataWriter,
}

impl Storage {
    pub fn new(path: PathBuf) -> Self {
        let writer =
            DataWriter::new(path, "main_storage").expect("Failed to initialize Storage writer.");
        let initial = writer.load().unwrap_or_default();

        Storage {
            inner: RwLock::new(initial),
            writer,
        }
    }

    pub fn set(&self, val: Value) -> Result<()> {
        let mut map = self.inner.write().unwrap();
        map.insert(val.key(), val);
        self.writer.write(&*map)?;
        Ok(())
    }

    pub fn get(&self, k: Key) -> Option<Value> {
        self.inner.read().unwrap().get(&k).cloned()
    }

    pub fn theme(&self) -> Theme {
        match self.get(Key::Theme) {
            Some(Value::Theme(t)) => t,
            _ => Theme::default(),
        }
    }

    pub fn registered(&self) -> bool {
        match self.get(Key::Registered) {
            Some(Value::Registered(b)) => b,
            _ => false,
        }
    }
}

#[derive(Default, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}
