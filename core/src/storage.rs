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
#[serde(untagged)]
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
        Storage {
            inner: RwLock::new(HashMap::new()),
            writer: DataWriter::new(path, "main_storage").unwrap(),
        }
    }
    pub fn set(&self, val: Value) -> Result<()> {
        let mut map = self.inner.write().unwrap();
        map.insert(val.key(), val);
        self.writer.write(&*map);
        Ok(())
    }
    pub fn get(&self, k: Key) -> Option<Value> {
        self.inner.read().unwrap().get(&k).cloned()
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
}
