mod access;
mod paths;
mod stream;
mod vault;

use crate::ServerCmd::ListVaults;
use crate::access::Access;
use crate::paths::app_data;
use crate::stream::StreamPair;
use crate::vault::{Vault, Vaults};
use anyhow::Context;
use anyhow::{Result, bail, ensure};
use iroh::endpoint::Connection;
use iroh::endpoint::QuicTransportConfig;
use iroh::endpoint::presets;
use iroh::protocol::AcceptError;
use iroh::protocol::ProtocolHandler;
use iroh::protocol::Router;
use iroh::{Endpoint, PublicKey};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

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
