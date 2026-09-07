use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Error;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct DataWriter {
    storage_path: PathBuf,
    write_lock: Arc<std::sync::Mutex<()>>,
}

impl DataWriter {
    pub fn storage_path(&self) -> PathBuf {
        self.storage_path.clone()
    }
    pub fn new(mut storage_path: PathBuf, store_name: &str) -> Result<Self, Error> {
        std::fs::create_dir_all(&storage_path)?;
        storage_path.push(format!("{store_name}.json"));

        // Clean stale tmp
        let tmp_path = storage_path.with_extension("json.tmp");
        if tmp_path.exists() {
            tracing::warn!("Stale tmp_store found, removing: {:#?}", tmp_path);
            let _ = std::fs::remove_file(&tmp_path);
        }

        Ok(Self {
            storage_path,
            write_lock: Arc::new(std::sync::Mutex::new(())),
        })
    }

    pub fn write<T: Serialize>(&self, data: &T) -> Result<usize, std::io::Error> {
        let _guard = self.write_lock.lock().unwrap();
        let json = serde_json::to_string(data).unwrap();
        let len = json.len();
        let tmp_path = self.storage_path.with_extension("json.tmp");

        std::fs::write(&tmp_path, &json)?;
        if let Err(e) = std::fs::rename(&tmp_path, &self.storage_path) {
            let _ = std::fs::remove_file(&tmp_path);
            tracing::error!("Failed to rename file {e}");
            return Err(e);
        }
        Ok(len)
    }
    pub fn wipe(&self) -> Result<(), Error> {
        tracing::trace!("Wiping {:#?}", self.storage_path);
        std::fs::remove_file(&self.storage_path)?;
        Ok(())
    }
    /// Load raw JSON from disk
    pub fn load(&self) -> Option<Value> {
        if self.storage_path.exists() {
            let content = std::fs::read_to_string(&self.storage_path).ok()?;
            tracing::trace!("Storage path {:?}", self.storage_path);
            serde_json::from_str(&content).ok()
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub struct KvStore {
    inner: Arc<std::sync::RwLock<HashMap<String, Value>>>,
    writer: DataWriter,
}

impl KvStore {
    pub fn new(storage_path: PathBuf, store_name: &str) -> Result<Self, Error> {
        let writer = DataWriter::new(storage_path, store_name)?;
        let initial: HashMap<String, Value> = writer
            .load()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        Ok(Self {
            inner: Arc::new(std::sync::RwLock::new(initial)),
            writer,
        })
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.inner.read().unwrap().get(key).cloned()
    }

    pub fn set(&self, key: &str, value: Value) -> Result<usize, Error> {
        let mut map = self.inner.write().unwrap();
        map.insert(key.to_string(), value);
        self.writer.write(&*map)
    }

    pub fn delete(&self, key: &str) -> Result<usize, Error> {
        let mut map = self.inner.write().unwrap();
        map.remove(key);
        self.writer.write(&*map)
    }

    pub fn wipe(&self) -> Result<(), Error> {
        *self.inner.write().unwrap() = HashMap::new();
        self.writer.wipe()
    }
    pub fn path(&self) -> PathBuf {
        self.writer.storage_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn write_read_delete() {
        let temp_dir = std::env::temp_dir();
        let store = KvStore::new(temp_dir, "test_store").expect("Failed to create KvStore");

        store
            .set("test", Value::Bool(true))
            .expect("Failed to set value");

        store.get("test").ok_or(()).expect("Failed to get value");

        assert!(store.get("test123").is_none(), "test123 wasn't None!");

        store.delete("test").expect("Failed to delete kv-pair");

        assert!(store.get("test").is_none(), "test wasn't None!");
        store.wipe().expect("Failed to wipe store");

        assert!(
            !std::fs::exists(store.path()).expect("Failed to open store file"),
            "Store still exists"
        );
    }
}
