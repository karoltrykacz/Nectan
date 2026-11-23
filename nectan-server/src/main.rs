mod access;
mod cache;
mod paths;
mod stream;
mod vault;

use crate::ServerCmd::ListVaults;
use crate::access::Access;
use crate::cache::{CACHE_ALPN, NectanCacheProtocol};
use crate::paths::app_data;
use crate::stream::StreamPair;
use crate::vault::{Vault, Vaults};
use anyhow::Context;
use anyhow::{Result, bail, ensure};
use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, AeadCore, Generate, Key, KeyInit},
};
use ed25519_dalek::{SigningKey, VerifyingKey};
use hkdf::Hkdf;
use iroh::endpoint::Connection;
use iroh::endpoint::QuicTransportConfig;
use iroh::endpoint::presets;
use iroh::protocol::ProtocolHandler;
use iroh::protocol::Router;
use iroh::protocol::{AcceptError, RouterBuilder};
use iroh::{Endpoint, PublicKey};
use rand::rand_core::UnwrapErr;
use rand::rngs::SysRng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub const ALPN: &[u8] = b"nectan/0";

pub type DeviceId = PublicKey;

#[derive(Debug)]
struct NectanServerProtocol {
    vaults: Arc<Vaults>,
}
impl NectanServerProtocol {
    fn new(v: Arc<Vaults>) -> Self {
        NectanServerProtocol { vaults: v }
    }
}

impl ProtocolHandler for NectanServerProtocol {
    async fn on_accepting(
        &self,
        accepting: iroh::endpoint::Accepting,
    ) -> Result<Connection, AcceptError> {
        let conn = accepting.await?;
        Ok(conn)
    }

    async fn accept(
        &self,
        connection: Connection,
    ) -> Result<(), AcceptError> {
        let remote_device = connection.remote_id();
        info!(
            "Accepting connection from {}",
            remote_device.to_string()
        );

        while let Ok(stream) = StreamPair::accept(&connection).await {
            tokio::spawn(handle_stream(stream, remote_device));
        }
        Ok(())
    }

    async fn shutdown(&self) {}
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ServerCmd {
    /// List vaults peer is entitled to see
    ListVaults,
}

async fn handle_stream(
    mut stream: StreamPair,
    remote_device: DeviceId,
) {
    info!("Handling new stream");
    let Ok(cmd) = stream.read().await else {
        // Connection should be closed here
        return;
    };

    match cmd {
        ListVaults => {
            info!(
                "Listing avaliable vaults for {}",
                remote_device.to_string()
            );
        }
    }
}

fn transport_config() -> QuicTransportConfig {
    QuicTransportConfig::builder()
        .stream_receive_window(32_000_000u32.into())
        .receive_window(128_000_000u32.into())
        .send_window(64_000_000u64)
        .initial_mtu(1200)
        .min_mtu(1200)
        .enable_segmentation_offload(true)
        .max_idle_timeout(Some(
            Duration::from_secs(20).try_into().unwrap(),
        ))
        .keep_alive_interval(Duration::from_secs(5))
        .build()
}

async fn start_server() -> anyhow::Result<()> {
    let builder = Endpoint::builder(presets::N0);
    let builder = builder.transport_config(transport_config());
    let endpoint =
        builder.bind().await.expect("Failed to bind endpoint");
    let id = endpoint.id();

    let vaults = Arc::new(Vaults::load()?);
    let server = NectanServerProtocol::new(vaults);
    let router =
        Router::builder(endpoint).accept(ALPN, server).spawn();

    info!("Router spawned");

    let access =
        Access::load(&app_data()).expect("Failed to load allowlist.");
    access.update(id, true).unwrap();
    tokio::time::sleep(Duration::from_hours(1)).await;

    Ok(())
}
fn encode_base64(bytes: impl AsRef<[u8]>) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD.encode(bytes.as_ref())
}

fn decode_base64(key: SigningKey) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD.encode(key.to_bytes())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    // start_server();
    // Storage::new();

    let endpoint = Endpoint::builder(presets::N0)
        .bind()
        .await
        .expect("Failed to bind endpoint");

    let cache = NectanCacheProtocol::load();
    let router =
        RouterBuilder::new(endpoint).accept(CACHE_ALPN, cache);
    tokio::signal::ctrl_c().await;

    Ok(())
}
//
// #[derive(Debug)]
// pub struct Link {
//     parent_id: Uuid,
//     id: Uuid,
// }

#[derive(Debug)]
enum NodeType {
    Directory,
    File,
}

#[derive(Debug)]
pub struct FileNode {}

#[derive(Debug)]
pub struct DirNode {}

#[derive(Debug)]
pub enum NodeKind {
    File(FileNode),
    Dir(DirNode),
}

#[derive(Debug)]
pub enum StorageError {
    ParentNotFound(Uuid),
    ParentNotDir(Uuid),
}

#[derive(Debug)]
pub struct Node {
    pub id: Uuid,
    pub parent: Uuid,
    pub kind: NodeKind,
}

struct Storage {
    /// Map of all nodes
    nodes: HashMap<Uuid, Node>,
    /// Parent -> children ids
    children: HashMap<Uuid, HashSet<Uuid>>,
    database: sqlite::Connection,
}

// impl Storage {
//     fn new() {
//         let connection = sqlite::open("user1").unwrap();
//         println!("Connecion opened");
//
//
//         let query = "SELECT * FROM users WHERE age > 50";
//
//         connection
//             .iterate(query, |pairs| {
//                 for &(name, value) in pairs.iter() {
//                     println!("{} = {}", name, value.unwrap());
//                 }
//                 true
//             })
//             .unwrap();
//     }
//     fn add_dir_root(&mut self) -> Uuid {
//         let id = Uuid::new_v4();
//         self.nodes.insert(
//             Uuid::default(),
//             Node {
//                 id,
//                 parent: Uuid::default(),
//                 kind: NodeKind::Dir(DirNode {}),
//             },
//         );
//         id
//     }
//
//     pub fn add_file(
//         &mut self,
//         parent: Uuid,
//         file: FileNode,
//     ) -> Result<Uuid, StorageError> {
//         self.add_child(parent, NodeKind::File(file))
//     }
//
//     pub fn add_dir(
//         &mut self,
//         parent: Uuid,
//         name: String,
//     ) -> Result<Uuid, StorageError> {
//         self.add_child(parent, NodeKind::Dir(DirNode {}))
//     }
//
//     fn add_child(
//         &mut self,
//         parent: Uuid,
//         kind: NodeKind,
//     ) -> Result<Uuid, StorageError> {
//         let id = Uuid::new_v4();
//
//         // Register in the parent first; this borrow ends at the closing brace.
//         {
//             let parent_node = self
//                 .nodes
//                 .get_mut(&parent)
//                 .ok_or(StorageError::ParentNotFound(parent))?;
//
//             match &mut parent_node.kind {
//                 NodeKind::Dir(dir) => {}
//
//                 NodeKind::File(_) => {
//                     return Err(StorageError::ParentNotDir(parent));
//                 }
//             }
//         }
//
//         self.nodes.insert(id, Node { id, parent, kind });
//         Ok(id)
//     }
// }
