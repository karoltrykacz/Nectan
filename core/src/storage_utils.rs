use core::fmt;
use core::fmt::Debug;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Display;
use std::io::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use tracing::info;

pub trait Serializer: Default {
    fn serialize<V: Serialize>(&self, v: &V) -> Option<Vec<u8>>;
}

pub trait Deserializer: Default {
    fn deserialize<V: DeserializeOwned>(&self, data: &[u8]) -> Option<V>;
}

#[derive(Default)]
pub struct JsonSerializer;

impl Serializer for JsonSerializer {
    fn serialize<V: Serialize>(&self, v: &V) -> Option<Vec<u8>> {
        serde_json::to_vec(v).ok()
    }
}
impl Deserializer for JsonSerializer {
    fn deserialize<V: DeserializeOwned>(&self, data: &[u8]) -> Option<V> {
        serde_json::from_slice(data).ok()
    }
}

#[derive(Default)]
pub struct PostcardSerializer;

impl Serializer for PostcardSerializer {
    fn serialize<V: Serialize>(&self, v: &V) -> Option<Vec<u8>> {
        postcard::to_allocvec(v).ok()
    }
}

impl Deserializer for PostcardSerializer {
    fn deserialize<V: DeserializeOwned>(&self, data: &[u8]) -> Option<V> {
        postcard::from_bytes(data).ok()
    }
}

#[derive(Debug)]
pub(crate) struct DataWriter<S: Serializer = JsonSerializer, D: Deserializer = JsonSerializer> {
    storage_path: PathBuf,
    write_lock: Arc<Mutex<()>>,
    pub serializer: S,
    pub deserializer: D,
}

impl<S: Serializer, D: Deserializer> DataWriter<S, D> {
    pub fn storage_path(&self) -> PathBuf {
        self.storage_path.clone()
    }
    pub fn new(mut storage_path: PathBuf, store_name: &str) -> Result<Self, Error> {
        std::fs::create_dir_all(&storage_path)?;
        storage_path.push(format!("{store_name}.json"));

        let tmp_path = storage_path.with_extension("json.tmp");

        if tmp_path.exists() {
            let _ = std::fs::remove_file(&tmp_path);
        }

        Ok(Self {
            storage_path,
            write_lock: Arc::new(std::sync::Mutex::new(())),
            serializer: S::default(),
            deserializer: D::default(),
        })
    }

    pub fn write<T: Serialize>(&self, data: &T) -> Result<usize, std::io::Error> {
        let _guard = self.write_lock.lock().unwrap();

        let data = self.serializer.serialize(data).unwrap();
        let len = data.len();

        let tmp_path = self.storage_path.with_extension("temp_data.tmp");

        std::fs::write(&tmp_path, &data)?;
        if let Err(e) = std::fs::rename(&tmp_path, &self.storage_path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }
        Ok(len)
    }
    pub fn wipe(&self) -> Result<(), Error> {
        std::fs::remove_file(&self.storage_path)?;
        Ok(())
    }
    pub fn load_raw(&self) -> Option<Vec<u8>> {
        std::fs::read(&self.storage_path).ok()
    }

    pub fn load<V: DeserializeOwned>(&self) -> Option<V> {
        let data = std::fs::read(&self.storage_path).ok()?;
        self.deserializer.deserialize(&data)
    }
}

pub struct KvStore<V, S: Serializer = JsonSerializer, D: Deserializer = JsonSerializer> {
    inner: Arc<RwLock<HashMap<String, V>>>,
    writer: DataWriter<S, D>,
}

impl<V: Clone + Serialize + DeserializeOwned, S: Serializer, D: Deserializer> KvStore<V, S, D> {
    pub fn new(storage_path: PathBuf, store_name: &str) -> Result<Self, Error>
    where
        S: Default,
        D: Default,
    {
        let writer = DataWriter::new(storage_path, store_name)?;
        let initial: HashMap<String, V> = writer.load().unwrap_or_default();

        Ok(Self {
            inner: Arc::new(RwLock::new(initial)),
            writer,
        })
    }

    pub fn get(&self, key: &str) -> Option<V> {
        self.inner.read().unwrap().get(key).cloned()
    }

    pub fn set(&self, key: &str, value: V) -> Result<usize, Error> {
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

    pub fn len(&self) -> usize {
        self.inner.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.read().unwrap().is_empty()
    }

    pub fn keys(&self) -> Vec<String> {
        self.inner.read().unwrap().keys().cloned().collect()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.inner.read().unwrap().contains_key(key)
    }
}

pub(crate) fn _convert<T, S1, D1, S2, D2>(
    old: &DataWriter<S1, D1>,
    new: &DataWriter<S2, D2>,
) -> Result<(), Error>
where
    T: Serialize + DeserializeOwned,
    S1: Serializer,
    D1: Deserializer,
    S2: Serializer,
    D2: Deserializer,
{
    let raw = std::fs::read(old.storage_path())?;
    let data: T = old
        .deserializer
        .deserialize(&raw)
        .ok_or_else(|| Error::new(std::io::ErrorKind::InvalidData, "deserialize failed"))?;

    new.write(&data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let mut dir = std::env::temp_dir();
        dir.push(format!("kvfix_test_{tag}_{nanos}"));
        dir
    }

    fn cleanup(dir: &PathBuf) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn data_writer_write_then_load_roundtrip() {
        let dir = scratch_dir("dw_roundtrip");
        let writer: DataWriter = DataWriter::new(dir.clone(), "store").unwrap();

        let mut map = HashMap::new();
        map.insert("a".to_string(), 1u32);
        map.insert("b".to_string(), 2u32);

        writer.write(&map).expect("write should succeed");

        let loaded: HashMap<String, u32> = writer.load().expect("load should succeed");
        assert_eq!(loaded, map);

        cleanup(&dir);
    }

    #[test]
    fn data_writer_load_missing_file_returns_none() {
        let dir = scratch_dir("dw_missing");
        let writer: DataWriter = DataWriter::new(dir.clone(), "store").unwrap();
        let loaded: Option<HashMap<String, u32>> = writer.load();

        assert!(loaded.is_none());

        cleanup(&dir);
    }

    #[test]
    fn data_writer_overwrite_replaces_old_contents() {
        let dir = scratch_dir("dw_overwrite");
        let writer: DataWriter = DataWriter::new(dir.clone(), "store").unwrap();

        let mut first = HashMap::new();
        first.insert("x".to_string(), 10u32);
        writer.write(&first).unwrap();

        let mut second = HashMap::new();
        second.insert("y".to_string(), 20u32);
        writer.write(&second).unwrap();

        let loaded: HashMap<String, u32> = writer.load().unwrap();
        assert_eq!(loaded, second);
        assert!(!loaded.contains_key("x"));

        cleanup(&dir);
    }

    #[test]
    fn data_writer_wipe_removes_file() {
        let dir = scratch_dir("dw_wipe");
        let writer: DataWriter = DataWriter::new(dir.clone(), "store").unwrap();

        let mut map = HashMap::new();
        map.insert("a".to_string(), 1u32);
        writer.write(&map).unwrap();

        assert!(writer.storage_path().exists());
        writer.wipe().expect("wipe should succeed");
        assert!(!writer.storage_path().exists());

        cleanup(&dir);
    }

    #[test]
    fn data_writer_no_leftover_tmp_file_after_write() {
        let dir = scratch_dir("dw_tmp_cleanup");
        let writer: DataWriter = DataWriter::new(dir.clone(), "store").unwrap();

        let mut map = HashMap::new();
        map.insert("a".to_string(), 1u32);
        writer.write(&map).unwrap();

        let tmp_path = writer.storage_path().with_extension("temp_data.tmp");
        assert!(
            !tmp_path.exists(),
            "temp file should be renamed away, not left behind"
        );

        cleanup(&dir);
    }

    #[test]
    fn data_writer_stale_tmp_file_is_cleared_on_new() {
        let dir = scratch_dir("dw_stale_tmp");
        std::fs::create_dir_all(&dir).unwrap();
        let mut storage_path = dir.clone();
        storage_path.push("store.json");
        let stale_tmp = storage_path.with_extension("json.tmp");
        std::fs::write(&stale_tmp, b"garbage").unwrap();

        let _writer: DataWriter = DataWriter::new(dir.clone(), "store").unwrap();
        assert!(!stale_tmp.exists());

        cleanup(&dir);
    }

    #[test]
    fn postcard_serializer_roundtrip() {
        let dir = scratch_dir("dw_postcard");
        let writer: DataWriter<PostcardSerializer, PostcardSerializer> =
            DataWriter::new(dir.clone(), "store").unwrap();

        let mut map = HashMap::new();
        map.insert("a".to_string(), 42u32);
        writer.write(&map).unwrap();

        let loaded: HashMap<String, u32> = writer.load().unwrap();
        assert_eq!(loaded, map);

        cleanup(&dir);
    }

    #[test]
    fn kvstore_set_and_get() {
        let dir = scratch_dir("kv_set_get");
        let store: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();

        store.set("k1", 100).unwrap();
        assert_eq!(store.get("k1"), Some(100));
        assert_eq!(store.get("missing"), None);

        cleanup(&dir);
    }

    #[test]
    fn kvstore_overwrite_existing_key() {
        let dir = scratch_dir("kv_overwrite_key");
        let store: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();

        store.set("k1", 1).unwrap();
        store.set("k1", 2).unwrap();
        assert_eq!(store.get("k1"), Some(2));
        assert_eq!(store.len(), 1);

        cleanup(&dir);
    }

    #[test]
    fn kvstore_delete_removes_key() {
        let dir = scratch_dir("kv_delete");
        let store: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();

        store.set("k1", 1).unwrap();
        store.delete("k1").unwrap();
        assert_eq!(store.get("k1"), None);
        assert!(store.is_empty());

        cleanup(&dir);
    }

    #[test]
    fn kvstore_delete_missing_key_is_noop_ok() {
        let dir = scratch_dir("kv_delete_missing");
        let store: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();

        store.set("k1", 1).unwrap();
        // deleting a key that was never there shouldn't error or touch k1
        store.delete("nope").unwrap();
        assert_eq!(store.get("k1"), Some(1));

        cleanup(&dir);
    }

    #[test]
    fn kvstore_wipe_clears_memory_and_disk() {
        let dir = scratch_dir("kv_wipe");
        let store: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();

        store.set("k1", 1).unwrap();
        store.wipe().unwrap();

        assert!(store.is_empty());
        assert!(!store.path().exists());

        cleanup(&dir);
    }

    #[test]
    fn kvstore_reopen_loads_previously_persisted_data() {
        let dir = scratch_dir("kv_reopen");
        {
            let store: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
            store.set("a", 1).unwrap();
            store.set("b", 2).unwrap();
        }

        let reopened: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
        assert_eq!(reopened.get("a"), Some(1));
        assert_eq!(reopened.get("b"), Some(2));
        assert_eq!(reopened.len(), 2);

        cleanup(&dir);
    }

    #[test]
    fn kvstore_reopen_with_no_prior_file_starts_empty() {
        let dir = scratch_dir("kv_reopen_empty");
        let store: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
        assert!(store.is_empty());
        assert_eq!(store.get("anything"), None);

        cleanup(&dir);
    }

    #[test]
    fn kvstore_set_after_reopen_adds_to_existing_data() {
        let dir = scratch_dir("kv_reopen_then_set");
        {
            let store: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
            store.set("a", 1).unwrap();
        }

        let store2: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
        store2.set("b", 2).unwrap();

        assert_eq!(store2.get("a"), Some(1));
        assert_eq!(store2.get("b"), Some(2));

        let store3: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
        assert_eq!(store3.len(), 2);

        cleanup(&dir);
    }

    #[test]
    fn kvstore_delete_persists_across_reopen() {
        let dir = scratch_dir("kv_delete_persist");
        {
            let store: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
            store.set("a", 1).unwrap();
            store.set("b", 2).unwrap();
            store.delete("a").unwrap();
        }

        let reopened: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
        assert_eq!(reopened.get("a"), None);
        assert_eq!(reopened.get("b"), Some(2));

        cleanup(&dir);
    }

    #[test]
    fn kvstore_wipe_then_reopen_starts_empty() {
        let dir = scratch_dir("kv_wipe_reopen");
        {
            let store: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
            store.set("a", 1).unwrap();
            store.wipe().unwrap();
        }

        let reopened: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
        assert!(reopened.is_empty());

        cleanup(&dir);
    }

    #[test]
    fn kvstore_works_with_struct_values() {
        #[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
        struct Person {
            name: String,
            age: u64,
        }

        let dir = scratch_dir("kv_struct_values");
        {
            let store: KvStore<Person> = KvStore::new(dir.clone(), "store").unwrap();
            store
                .set(
                    "karol",
                    Person {
                        name: "Karol".to_string(),
                        age: 19,
                    },
                )
                .unwrap();
        }

        let reopened: KvStore<Person> = KvStore::new(dir.clone(), "store").unwrap();
        assert_eq!(
            reopened.get("karol"),
            Some(Person {
                name: "Karol".to_string(),
                age: 19
            })
        );

        cleanup(&dir);
    }

    #[test]
    fn kvstore_keys_and_contains_key() {
        let dir = scratch_dir("kv_keys");
        let store: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
        store.set("a", 1).unwrap();
        store.set("b", 2).unwrap();

        let mut keys = store.keys();
        keys.sort();
        assert_eq!(keys, vec!["a".to_string(), "b".to_string()]);
        assert!(store.contains_key("a"));
        assert!(!store.contains_key("z"));

        cleanup(&dir);
    }

    #[test]
    fn kvstore_multiple_instances_same_path_reflect_persisted_writes() {
        let dir = scratch_dir("kv_multi_instance");
        let store1: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
        store1.set("a", 1).unwrap();

        let store2: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
        assert_eq!(store2.get("a"), Some(1));

        store2.set("b", 2).unwrap();
        let store3: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
        assert_eq!(store3.len(), 2);

        cleanup(&dir);
    }

    #[test]
    fn kvstore_corrupted_file_falls_back_to_empty_not_panic() {
        let dir = scratch_dir("kv_corrupted");
        std::fs::create_dir_all(&dir).unwrap();
        let mut path = dir.clone();
        path.push("store.json");
        std::fs::write(&path, b"not valid json {{{").unwrap();

        let store: KvStore<u32> = KvStore::new(dir.clone(), "store").unwrap();
        assert!(store.is_empty());

        cleanup(&dir);
    }
}

#[derive(Serialize, Deserialize)]
struct Siema {
    username: String,
    age: u64,
}

impl Display for Siema {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Username {}", self.username)
    }
}

fn main() {
    let s = Siema {
        username: String::from("Karol"),
        age: 19,
    };
    println!("Siema - {s}");
}
