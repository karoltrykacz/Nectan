use crate::{
    devices::{DeviceId, Devices, UserInfo},
    messages::AppEvent,
    protocol::{ALPN, NectanProtocol, NectanState, announce_endpoint, start_registration_loop},
    storage_utils::KvStore,
};
use anyhow::Result;
use ed25519_dalek::SigningKey;
use iroh::{
    Endpoint,
    endpoint::{QuicTransportConfig, presets},
    protocol::Router,
};
use iroh_mdns_address_lookup::MdnsAddressLookup;
use std::{sync::Arc, time::Duration};

pub mod code_lookup;
pub mod common;
pub mod devices;
pub mod format;
pub mod messages;
pub mod path_tree;
pub mod protocol;
pub mod storage_utils;
pub mod stream;
pub mod transfers;
pub mod walker;

pub async fn setup_core(
    app_event_tx: tokio::sync::mpsc::Sender<AppEvent>,
    device_id: DeviceId,
    userinfo: UserInfo,
    signing_key: SigningKey,
    devices: Devices,
    store: KvStore,
) -> Result<NectanState> {
    let transport = QuicTransportConfig::builder()
        .stream_receive_window(32_000_000u32.into())
        .receive_window(128_000_000u32.into())
        .send_window(64_000_000u64)
        .initial_mtu(1200)
        .min_mtu(1200)
        .enable_segmentation_offload(true)
        .max_idle_timeout(Some(Duration::from_secs(20).try_into().unwrap()))
        .keep_alive_interval(Duration::from_secs(5))
        .build();

    let endpoint = Endpoint::builder(presets::N0)
        .transport_config(transport)
        .bind()
        .await?;

    let mdns = MdnsAddressLookup::builder()
        .service_name("nectan_user")
        .advertise(true)
        .build(endpoint.id())
        .unwrap();

    endpoint.address_lookup().unwrap().add(mdns.clone());

    let state = NectanState::build(
        userinfo,
        device_id,
        signing_key,
        devices,
        app_event_tx,
        store,
    )
    .await;
    let prot = NectanProtocol::new(endpoint.clone(), Arc::new(state.clone()));
    let router = Router::builder(endpoint).accept(ALPN, prot).spawn();
    state.attach_router(&router);

    start_registration_loop(&state);
    announce_endpoint(&state);

    Ok(state)
}
