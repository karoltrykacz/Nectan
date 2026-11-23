//! Nectan Cache Server

use crate::stream::StreamPair;
use anyhow::{Context, bail};
use blake3::Hash;
use iroh::{
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    fs::File,
    io::{AsyncWriteExt, BufReader, BufWriter},
    sync::watch,
};
use tokio_util::io::InspectReader;
use tracing::{error, info, warn};

pub const CACHE_ALPN: &[u8] = b"nectan/cache-server/0";

const HALF_LIFE_SECS: u64 = 86_400;

#[derive(Debug)]
pub struct NectanCacheProtocol {
    index: Mutex<CacheIndex>,
    cache_dir: PathBuf,
    config_path: PathBuf,
    cache_cfg: watch::Sender<CacheConfig>,
}

impl ProtocolHandler for NectanCacheProtocol {
    async fn accept(
        &self,
        connection: Connection,
    ) -> Result<(), AcceptError> {
        if let Err(e) = self.handle_connection(connection).await {
            warn!("Cache connection ended with error: {e:#}");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum CacheCommand {
    /// Purge all items from cache
    Purge,
    /// Put new item
    Put,
    /// Retrieve item from cache
    Get,
    /// Change cache configuration
    Configure,
    /// Warm file
    Warm,
    ReadConfig,
}

/// General set of protocol responses
#[derive(Debug, Serialize, Deserialize)]
enum CacheResponse {
    Ok,
    Hit,
    Miss,
    Bad,
    TooBig,
    AlreadyCached,
    Error,
}

/// Cache eviction policies
/// Default one is Frecency Based Algorithm. (Strongly recommended)
/// Frecency combines the frequency and recency into a single measure.
#[derive(Default, Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum CacheEvictionPolicy {
    /// Frecency based algorithm.
    #[default]
    FRE,
    /// Least Recently Used item
    LRU,
    /// Most Recently Used
    MRU,
    /// Time To Live
    TTL,
    /// Least Frequently Used
    LFU,
    /// First In, First Out
    FIFO,
    /// Random.
    Random,
}

impl CacheEvictionPolicy {
    fn as_str(&self) -> &'static str {
        match self {
            Self::FRE => "fre",
            Self::LRU => "lru",
            Self::MRU => "mru",
            Self::TTL => "ttl",
            Self::LFU => "lfu",
            Self::FIFO => "fifo",
            Self::Random => "random",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Default, Deserialize)]
enum SizeLimit {
    #[default]
    NoLimit,
    Limit(u64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheConfig {
    /// Maximum size of single item
    item_size_limit: SizeLimit,
    /// Maximum size of cache
    disk_max_size: SizeLimit,
    eviction_policy: CacheEvictionPolicy,
    /// Max age of an item in seconds (only used by the TTL policy, 0 = never)
    ttl_secs: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            item_size_limit: SizeLimit::NoLimit,
            disk_max_size: SizeLimit::NoLimit,
            eviction_policy: CacheEvictionPolicy::default(),
            ttl_secs: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct EntryMeta {
    size: u64,
    /// Unix seconds
    inserted: u64,
    /// Unix seconds
    last_access: u64,
    hits: u64,
}

#[derive(Debug, Default)]
struct CacheIndex {
    entries: HashMap<Hash, EntryMeta>,
    total_size: u64,
}

impl CacheIndex {
    fn insert(&mut self, hash: Hash, meta: EntryMeta) {
        let size = meta.size;
        if let Some(old) = self.entries.insert(hash, meta) {
            self.total_size =
                self.total_size.saturating_sub(old.size);
        }
        self.total_size += size;
    }

    fn remove(&mut self, hash: &Hash) -> Option<EntryMeta> {
        let meta = self.entries.remove(hash)?;
        self.total_size = self.total_size.saturating_sub(meta.size);
        Some(meta)
    }

    /// Record an access.
    /// #Returns false when the item is unknown.
    fn touch(&mut self, hash: &Hash, now: u64) -> bool {
        match self.entries.get_mut(hash) {
            Some(m) => {
                m.hits += 1;
                m.last_access = now;
                true
            }
            None => false,
        }
    }

    /// Choose the next item to evict according to `cfg.eviction_policy`.
    fn pick_victim(
        &self,
        protect: Option<Hash>,
        cfg: &CacheConfig,
        now: u64,
    ) -> Option<Hash> {
        let candidates =
            self.entries.iter().filter(|(h, _)| Some(**h) != protect);

        let picked = match cfg.eviction_policy {
            CacheEvictionPolicy::LRU => {
                candidates.min_by_key(|(_, m)| m.last_access)
            }
            CacheEvictionPolicy::MRU => {
                candidates.max_by_key(|(_, m)| m.last_access)
            }
            CacheEvictionPolicy::LFU => candidates
                .min_by_key(|(_, m)| (m.hits, m.last_access)),
            CacheEvictionPolicy::FIFO | CacheEvictionPolicy::TTL => {
                candidates.min_by_key(|(_, m)| m.inserted)
            }
            CacheEvictionPolicy::Random => {
                candidates.choose(&mut rand::rng())
            }
            CacheEvictionPolicy::FRE => candidates.min_by(|a, b| {
                frecency(a.1, now).total_cmp(&frecency(b.1, now))
            }),
        };
        picked.map(|(h, _)| *h)
    }
}

/// Frecency score
/// Access count that decays exponentially with time since the last access.
fn frecency(m: &EntryMeta, now: u64) -> f64 {
    let age = now.saturating_sub(m.last_access) as f64;
    (m.hits + 1) as f64 * 0.5f64.powf(age / HALF_LIFE_SECS as f64)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A size in the TOML file.
/// Plain bytes (`696969`) or text (`"69 GiB"`).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawSize {
    Bytes(u64),
    Text(String),
}

impl Default for RawSize {
    fn default() -> Self {
        RawSize::Bytes(0)
    }
}

impl RawSize {
    fn resolve(&self) -> anyhow::Result<SizeLimit> {
        let n = match self {
            RawSize::Bytes(n) => *n,
            RawSize::Text(s) => parse_size(s)?,
        };
        Ok(if n == 0 {
            SizeLimit::NoLimit
        } else {
            SizeLimit::Limit(n)
        })
    }
}

fn parse_size(s: &str) -> anyhow::Result<u64> {
    let s = s.trim();
    let split = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let num: f64 =
        num.parse().with_context(|| format!("invalid size {s:?}"))?;
    let mult: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "kb" => 1_000,
        "mb" => 1_000_000,
        "gb" => 1_000_000_000,
        "tb" => 1_000_000_000_000,
        "kib" | "k" => 1 << 10,
        "mib" | "m" => 1 << 20,
        "gib" | "g" => 1 << 30,
        "tib" | "t" => 1 << 40,
        other => bail!("unknown size unit {other:?} in {s:?}"),
    };
    Ok((num * mult as f64) as u64)
}

fn render_size(l: &SizeLimit) -> String {
    match l {
        SizeLimit::NoLimit => "0".into(),
        SizeLimit::Limit(n) => {
            for (unit, m) in [
                ("TiB", 1u64 << 40),
                ("GiB", 1 << 30),
                ("MiB", 1 << 20),
                ("KiB", 1 << 10),
            ] {
                if n % m == 0 {
                    return format!("\"{} {unit}\"", n / m);
                }
            }
            n.to_string()
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileLimits {
    item_size_limit: RawSize,
    disk_max_size: RawSize,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileEviction {
    policy: CacheEvictionPolicy,
    ttl_secs: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileConfig {
    limits: FileLimits,
    eviction: FileEviction,
}

impl CacheConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if let (SizeLimit::Limit(item), SizeLimit::Limit(disk)) =
            (self.item_size_limit, self.disk_max_size)
            && item > disk
        {
            bail!(
                "item_size_limit ({item}) is larger than disk_max_size ({disk})"
            );
        }
        Ok(())
    }

    fn from_toml(text: &str) -> anyhow::Result<Self> {
        let raw: FileConfig = toml::from_str(text)?;
        let cfg = CacheConfig {
            item_size_limit: raw.limits.item_size_limit.resolve()?,
            disk_max_size: raw.limits.disk_max_size.resolve()?,
            eviction_policy: raw.eviction.policy,
            ttl_secs: raw.eviction.ttl_secs,
        };
        cfg.validate()?;
        Ok(cfg)
    }

    /// Load the config and create the documented config file if missing.
    /// If file is broken, uses default vaules.
    fn load_or_create(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => match Self::from_toml(&text) {
                Ok(cfg) => cfg,
                Err(e) => {
                    error!(
                        "Invalid cache config {}. {e:#}. Using defaults.",
                        path.display()
                    );
                    Self::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let cfg = Self::default();
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) =
                    std::fs::write(path, render_config(&cfg))
                {
                    warn!(
                        "Could not write default cache config. {e}"
                    );
                }
                cfg
            }
            Err(e) => {
                warn!(
                    "Could not read cache config. {e}. Using defaults."
                );
                Self::default()
            }
        }
    }
}

/// Render the config as a fully documented TOML file.
/// Used for the initial file and whenever `Configure` persists a new config.
fn render_config(cfg: &CacheConfig) -> String {
    let item = render_size(&cfg.item_size_limit);
    let disk = render_size(&cfg.disk_max_size);
    let policy = cfg.eviction_policy.as_str();
    let ttl = cfg.ttl_secs;
    format!(
        r#"# Nectan Cache Server configuration
[limits]
# Maximum size of a single cached item. 
# Examples: 0 (unlimited), "256 MiB", "1 GiB", "100TiB"
item_size_limit = {item}

# Maximum total size of the cache directory. 
# Must not be smaller than item_size_limit.
# Examples: 0 (unlimited), "20 GiB"
disk_max_size = {disk}

[eviction]
#   "fre"    Frecency based algorithm (default and strongly recommended).
#   "lru"    Least Recently Used
#   "mru"    Most Recently Used
#   "lfu"    Least Frequently Used
#   "fifo"   Firt In, First Out 
#   "ttl"    Time To Live
#   "random" 
policy = "{policy}"

# Maximum age of an item in seconds. Only used by the "ttl" policy.
# 0 disables expiry.
ttl_secs = {ttl}
"#
    )
}

impl NectanCacheProtocol {
    pub fn load() -> Self {
        let cache_dir = dirs::cache_dir()
            .expect("Failed to resolve cache dir.")
            .join("NectanServer/cached");
        let config_path = dirs::config_dir()
            .expect("Failed to resolve config dir.")
            .join("NectanServer/cache-config.toml");

        std::fs::create_dir_all(&cache_dir)
            .expect("Failed to create cache dir.");

        // Rebuild the index from the files already on disk.
        let mut index = CacheIndex::default();
        if let Ok(rd) = std::fs::read_dir(&cache_dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                let Some(name) =
                    path.file_name().and_then(|n| n.to_str())
                else {
                    continue;
                };
                // Leftovers of interrupted uploads.
                if name.ends_with(".part") {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                let Ok(hash) = name.parse::<Hash>() else {
                    continue;
                };
                let Ok(md) = entry.metadata() else { continue };
                if !md.is_file() {
                    continue;
                }
                let ts = md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                index.insert(
                    hash,
                    EntryMeta {
                        size: md.len(),
                        inserted: ts,
                        last_access: ts,
                        hits: 0,
                    },
                );
            }
        }
        info!(
            "Cache index loaded: {} items, {} bytes.",
            index.entries.len(),
            index.total_size
        );

        let cfg = CacheConfig::load_or_create(&config_path);

        NectanCacheProtocol {
            index: Mutex::new(index),
            cache_dir,
            config_path,
            cache_cfg: watch::Sender::new(cfg),
        }
    }

    pub async fn handle_connection(
        &self,
        conn: Connection,
    ) -> anyhow::Result<()> {
        info!("Serving cache.");

        while let Ok(mut stream) = StreamPair::accept(&conn).await {
            let cmd: CacheCommand = match stream.read().await {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to read command: {e:#}");
                    continue;
                }
            };
            let result = match cmd {
                CacheCommand::Configure => {
                    self.configure(stream).await
                }
                CacheCommand::Put => self.put(stream).await,
                CacheCommand::Get => self.serve(stream).await,
                CacheCommand::Purge => self.purge(stream).await,
                CacheCommand::Warm => self.warm(stream).await,
                CacheCommand::ReadConfig => {
                    self.read_config(stream).await
                }
            };
            // A failed command must not take the whole connection down.
            if let Err(e) = result {
                warn!("{cmd:?} failed: {e:#}");
            }
        }

        Ok(())
    }

    fn cfg(&self) -> CacheConfig {
        self.cache_cfg.borrow().clone()
    }

    fn path_for(&self, hash: &Hash) -> PathBuf {
        self.cache_dir.join(hash.to_string())
    }

    /// Evicts items until the cache fits under `disk_max_size`
    async fn enforce_limits(&self, protect: Option<Hash>) {
        let cfg = self.cfg();
        let now = now_secs();
        let mut victims: Vec<Hash> = Vec::new();

        {
            let mut idx = self.index.lock().unwrap();

            if matches!(cfg.eviction_policy, CacheEvictionPolicy::TTL)
                && cfg.ttl_secs > 0
            {
                let expired: Vec<Hash> = idx
                    .entries
                    .iter()
                    .filter(|(h, m)| {
                        Some(**h) != protect
                            && now.saturating_sub(m.inserted)
                                > cfg.ttl_secs
                    })
                    .map(|(h, _)| *h)
                    .collect();
                for h in expired {
                    idx.remove(&h);
                    victims.push(h);
                }
            }

            if let SizeLimit::Limit(max) = cfg.disk_max_size {
                while idx.total_size > max {
                    let Some(v) = idx.pick_victim(protect, &cfg, now)
                    else {
                        break;
                    };
                    idx.remove(&v);
                    victims.push(v);
                }
            }
        }

        for h in victims {
            match tokio::fs::remove_file(self.path_for(&h)).await {
                Ok(()) => info!("Evicted {h}"),
                Err(e) => warn!("Failed to delete evicted {h}: {e}"),
            }
        }
    }

    /// Write the current config (with documentation) to disk.
    async fn persist_config(
        &self,
        cfg: &CacheConfig,
    ) -> anyhow::Result<()> {
        if let Some(parent) = self.config_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = self.config_path.with_extension("toml.part");
        tokio::fs::write(&tmp, render_config(cfg)).await?;
        tokio::fs::rename(&tmp, &self.config_path).await?;
        Ok(())
    }

    async fn warm(
        &self,
        mut stream: StreamPair,
    ) -> anyhow::Result<()> {
        let hash: Hash = stream.read().await?;

        if !self.index.lock().unwrap().entries.contains_key(&hash) {
            stream.write(&CacheResponse::Miss).await?;
            return Ok(());
        }

        let mut file = match File::open(self.path_for(&hash)).await {
            Ok(f) => f,
            Err(e) => {
                warn!("Warm: cannot open {hash}: {e}");
                stream.write(&CacheResponse::Error).await?;
                return Ok(());
            }
        };
        if let Err(e) =
            tokio::io::copy(&mut file, &mut tokio::io::sink()).await
        {
            warn!("Warm: cannot read {hash}: {e}");
            stream.write(&CacheResponse::Error).await?;
            return Ok(());
        }
        info!("Warmed {hash}");

        stream.write(&CacheResponse::Ok).await?;
        Ok(())
    }

    async fn put(
        &self,
        mut stream: StreamPair,
    ) -> anyhow::Result<()> {
        // Read hash of the data, peer wants to store
        let item_hash: Hash = stream.read().await?;
        if self.index.lock().unwrap().entries.contains_key(&item_hash)
        {
            stream.write(&CacheResponse::AlreadyCached).await?;
            bail!("Item already cached")
        }
        let size: u64 = stream.read().await?;
        let cfg = self.cfg();

        if let SizeLimit::Limit(max) = cfg.item_size_limit
            && size > max
        {
            stream.write(&CacheResponse::TooBig).await?;
            bail!("Item too large {size}");
        }

        // An item bigger than the whole cache could never be kept.
        if let SizeLimit::Limit(max) = cfg.disk_max_size
            && size > max
        {
            stream.write(&CacheResponse::TooBig).await?;
            bail!("Item {size} exceeds cache size {max}");
        }

        stream.write(&CacheResponse::Ok).await?;

        tokio::fs::create_dir_all(&self.cache_dir).await?;
        let final_path = self.path_for(&item_hash);
        let tmp_path =
            self.cache_dir.join(format!("{item_hash}.part"));

        // Receive bytes and validate the contents with blake3.
        let mut hasher = blake3::Hasher::new();
        let mut rx = stream.rx();

        let body = BufReader::with_capacity(256 * 1024, &mut rx);
        let mut body = InspectReader::new(body, |chunk| {
            hasher.update(chunk);
        });

        let mut file = BufWriter::new(File::create(&tmp_path).await?);
        let copied = tokio::io::copy(&mut body, &mut file).await;
        let copied = match copied {
            Ok(n) => n,
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err(e.into());
            }
        };
        file.shutdown().await?;

        drop(body);

        if copied != size {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            let _ = stream.write(&CacheResponse::Bad).await;
            bail!("Expected {size} bytes, got {copied}");
        }
        if hasher.finalize().as_bytes() != item_hash.as_bytes() {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            let _ = stream.write(&CacheResponse::Bad).await;
            bail!("Hash mismatch");
        }
        tokio::fs::rename(&tmp_path, &final_path).await?;

        let now = now_secs();
        self.index.lock().unwrap().insert(
            item_hash,
            EntryMeta {
                size,
                inserted: now,
                last_access: now,
                hits: 0,
            },
        );
        self.enforce_limits(Some(item_hash)).await;

        stream.write(&CacheResponse::Ok).await?;

        Ok(())
    }

    async fn serve(
        &self,
        mut stream: StreamPair,
    ) -> anyhow::Result<()> {
        // Read BLAKE3 hash of requested item
        let hash: Hash = stream.read().await?;

        let known =
            self.index.lock().unwrap().touch(&hash, now_secs());
        if !known {
            stream.write(&CacheResponse::Miss).await?;
            return Ok(());
        }

        let file = match File::open(self.path_for(&hash)).await {
            Ok(f) => f,
            Err(e) => {
                // Index and disk disagree: forget the item.
                warn!("Cached item {hash} missing on disk. {e}");
                self.index.lock().unwrap().remove(&hash);
                stream.write(&CacheResponse::Miss).await?;
                return Ok(());
            }
        };
        let size = file.metadata().await?.len();

        stream.write(&CacheResponse::Hit).await?;
        stream.write(&size).await?;

        // Stream back the file
        let mut reader = BufReader::with_capacity(256 * 1024, file);
        let mut tx = stream.tx();
        tokio::io::copy(&mut reader, &mut tx).await?;
        tx.flush().await?;

        Ok(())
    }

    async fn purge(
        &self,
        mut stream: StreamPair,
    ) -> anyhow::Result<()> {
        info!("Purging cache.");

        let result = match tokio::fs::remove_dir_all(&self.cache_dir)
            .await
        {
            Ok(()) => Ok(()),
            // Nothing to purge is fine.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(())
            }
            Err(e) => Err(e),
        };

        let result = match result {
            Ok(()) => {
                tokio::fs::create_dir_all(&self.cache_dir).await
            }
            Err(e) => Err(e),
        };

        match result {
            Ok(()) => {
                *self.index.lock().unwrap() = CacheIndex::default();
                info!("Cache purged successfully.");
                stream.write(&CacheResponse::Ok).await?;
            }
            Err(e) => {
                error!("Failed to purge cache. {e}");
                stream.write(&CacheResponse::Error).await?;
            }
        }

        Ok(())
    }

    async fn read_config(
        &self,
        mut stream: StreamPair,
    ) -> anyhow::Result<()> {
        let cfg = self.cfg();
        stream.write(&cfg).await?;
        Ok(())
    }

    async fn configure(
        &self,
        mut stream: StreamPair,
    ) -> anyhow::Result<()> {
        let new_cfg: CacheConfig = stream.read().await?;
        if let Err(e) = new_cfg.validate() {
            warn!("Rejected cache configuration: {e:#}");
            stream.write(&CacheResponse::Bad).await?;
            return Ok(());
        }

        self.cache_cfg.send_replace(new_cfg.clone());

        if let Err(e) = self.persist_config(&new_cfg).await {
            error!("Failed to persist cache config: {e:#}");
            stream.write(&CacheResponse::Error).await?;
            bail!("Failed to persist cache config: {e:#}")
        }

        // A smaller limit or a new policy may require evicting right away.
        self.enforce_limits(None).await;

        stream.write(&CacheResponse::Ok).await?;
        Ok(())
    }
}
