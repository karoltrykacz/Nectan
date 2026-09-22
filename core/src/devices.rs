use anyhow::{Result, anyhow, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use core::fmt;
use ed25519_dalek::VerifyingKey;
use iroh::{EndpointId, endpoint::Connection};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, RwLock},
};

use crate::{
    devices::DeviceStatus::Offline, storage_utils::DataWriter, transfers::PendingTransfers,
};

pub type DeviceId = VerifyingKey;

#[derive(Clone, Copy)]
pub enum DeviceStatus {
    Offline,
    Online,
    Nearby,
}

impl Default for DeviceStatus {
    fn default() -> Self {
        DeviceStatus::Offline
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Username {
    inner: String,
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Username {}", self.inner)
    }
}

impl Username {
    pub fn new(username: impl Into<String>) -> anyhow::Result<Self> {
        let username = username.into();
        let len = username.chars().count();
        if len < 3 {
            bail!("username too short. (Minimum 3 characters)");
        }
        if len > 24 {
            bail!("username too long (Maximum 24 characters)");
        }
        Ok(Self { inner: username })
    }
    pub fn to_string(&self) -> String {
        self.inner.clone()
    }
    pub fn as_string(self) -> String {
        self.inner
    }
}

impl Default for Username {
    fn default() -> Self {
        Username {
            inner: "Default".to_string(),
        }
    }
}

pub struct UserInfoInner {
    pub username: Username,
}

#[derive(Clone)]
pub struct UserInfo(Arc<std::sync::Mutex<UserInfoInner>>);

impl UserInfo {
    pub fn new(username: Username) -> Self {
        UserInfo(Arc::new(std::sync::Mutex::new(UserInfoInner { username })))
    }
    pub fn username(&self) -> Username {
        self.0.lock().unwrap().username.clone()
    }
    pub fn set_username(&self, username: Username) {
        self.0.lock().unwrap().username = username;
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Device {
    pub username: Username,
    #[serde(
        serialize_with = "serialize_device_id",
        deserialize_with = "deserialize_device_id"
    )]
    pub id: DeviceId,
    #[serde(skip)]
    pub connection: Option<Connection>,
    #[serde(skip)]
    pub status: DeviceStatus,
    pub deleted: bool,
    pub total_exchanged_data: u64,
    pub completed_transfers: u64,
    pub fav: bool,
    // #[serde(skip)]
    // pub pending_transfers: PendingTransfers,
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
// struct DevicesInner{
//     devices: RwLock<HashMap<DeviceId, Device>>,
//     nearby_endpoints: RwLock<HashMap<EndpointId, DeviceId>>,
//     writer: DataWriter,
// }

#[derive(Clone)]
pub struct Devices {
    inner: Arc<RwLock<HashMap<DeviceId, Device>>>,
    nearby_endpoints: Arc<RwLock<HashSet<EndpointId>>>,
    writer: DataWriter,
}

impl Devices {
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
            inner: Arc::new(RwLock::new(initial)),
            nearby_endpoints: Arc::new(RwLock::new(HashSet::new())),
            writer: store,
        })
    }

    fn persist(&self) -> Result<usize, std::io::Error> {
        let stored: Vec<Device> = {
            let lock = self.inner.read().unwrap();
            lock.values().cloned().collect()
        };
        self.writer.write(&stored)
    }
    pub fn get(&self, target: &DeviceId) -> Option<Device> {
        self.inner.read().unwrap().get(target).cloned()
    }
    pub fn get_all(&self) -> Vec<Device> {
        self.inner.read().unwrap().values().cloned().collect()
    }
    pub fn get_all_ids(&self) -> Vec<DeviceId> {
        self.inner.read().unwrap().keys().cloned().collect()
    }
    pub fn is_nearby(&self, endpoint_id: &EndpointId) -> bool {
        self.nearby_endpoints
            .read()
            .unwrap()
            .get(endpoint_id)
            .is_some()
    }
    pub fn remove_nearby(&self, endpoint_id: EndpointId) {
        self.nearby_endpoints.write().unwrap().remove(&endpoint_id);
    }
    /// Returns true if device is newly inserted
    pub fn add_nearby(&self, endpoint_id: EndpointId) -> bool {
        self.nearby_endpoints.write().unwrap().insert(endpoint_id)
    }
    pub fn insert(
        &self,
        target: DeviceId,
        device: &Device,
    ) -> Result<Option<Device>, std::io::Error> {
        let old = self
            .inner
            .write()
            .unwrap()
            .insert(target, device.to_owned());
        self.persist()?;
        Ok(old)
    }
    pub fn get_unresolved_devices(&self) -> Vec<DeviceId> {
        let mut unresolved_devices = Vec::new();
        let read_lock = self.inner.read().unwrap();
        for (device_id, device) in &*read_lock {
            if let None = device.connection
                && !device.deleted
            {
                unresolved_devices.push(*device_id);
            }
        }
        unresolved_devices
    }
    pub fn is_alive(&self, target: EndpointId) -> bool {
        self.inner.read().unwrap().iter().any(|(_, device)| {
            device
                .connection
                .as_ref()
                .is_some_and(|c| c.remote_id() == target && c.close_reason().is_none())
        })
    }
    pub fn remove_conn(&self, target: &DeviceId, conn_id: usize) -> bool {
        let mut lock = self.inner.write().unwrap();
        let Some(device) = lock.get_mut(&target) else {
            return false;
        };
        let Some(conn) = &device.connection else {
            return false;
        };
        if conn.stable_id() == conn_id {
            device.connection = None;
            device.status = Offline;
            return true;
        }
        false
    }
    pub fn get_connection(&self, device_id: DeviceId) -> Option<Connection> {
        self.inner
            .read()
            .unwrap()
            .get(&device_id)
            .and_then(|d| d.connection.clone())
    }
}

pub fn device_id_from_base64(s: &str) -> Result<DeviceId> {
    let bytes = STANDARD.decode(s)?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow!("Expected 32 bytes."))?;
    Ok(VerifyingKey::from_bytes(&arr)?)
}

pub fn device_id_to_base64(device_id: &DeviceId) -> String {
    STANDARD.encode(device_id.as_bytes())
}
