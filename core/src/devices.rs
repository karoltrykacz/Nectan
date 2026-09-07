use core::fmt;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};

use crate::{protocol::DeviceId, storage_utils::DataWriter};

#[derive(Serialize, Deserialize, Clone)]
struct Device {
    #[serde(
        serialize_with = "serialize_device_id",
        deserialize_with = "deserialize_device_id"
    )]
    id: DeviceId,
    username: String,
}

pub fn serialize_device_id<S>(id: &DeviceId, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    s.serialize_str(&STANDARD.encode(id.as_bytes()))
}

pub fn deserialize_device_id<'de, D>(d: D) -> Result<DeviceId, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    let bytes = STANDARD.decode(s).map_err(serde::de::Error::custom)?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| serde::de::Error::custom("invalid length"))?;
    VerifyingKey::from_bytes(&arr).map_err(serde::de::Error::custom)
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Device {}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Device")
            .field("username", &self.username)
            .finish()
    }
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Username {}", self.username)
    }
}

struct DevicesPoolInner {
    devices: std::sync::RwLock<HashMap<DeviceId, Device>>,
}

#[derive(Clone)]
pub struct DevicesPool {
    inner: Arc<DevicesPoolInner>,
    writer: DataWriter,
}

impl DevicesPool {
    pub fn new(path: Option<PathBuf>) -> Result<Self, std::io::Error> {
        let mut base_path = match path {
            Some(p) => p,
            // for windows and unix - on ios and androud the path should be provided
            None => dirs::data_dir()
                .expect("No path provided and system data directory could not be determined"),
        };

        base_path.push("Nectan");
        let store = DataWriter::new(base_path, "nectan_devices_store")?;

        let initial: HashMap<DeviceId, Device> = store
            .load()
            .and_then(|v| {
                let stored: Vec<Device> = serde_json::from_value(v).ok()?;
                Some(stored.into_iter().map(|s| (s.id, s)).collect())
            })
            .unwrap_or_default();

        Ok(Self {
            inner: Arc::new(DevicesPoolInner {
                devices: std::sync::RwLock::new(initial),
            }),
            writer: store,
        })
    }

    fn persist(&self) -> Result<usize, std::io::Error> {
        let stored: Vec<Device> = {
            let lock = self.inner.devices.read().unwrap();
            lock.values().cloned().collect()
        };
        self.writer.write(&stored)
    }
}
