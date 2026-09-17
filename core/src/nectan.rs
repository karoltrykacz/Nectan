use iroh::{Endpoint, EndpointAddr, Watcher, endpoint::presets, protocol::Router};
use nectan_core::{
    devices::{DevicesPool, UserInfo, Username},
    messages::{AppEvent, UiResponse},
    protocol::{ALPN, NectanProtocol, NectanState, connect, gen_device_id},
};
use std::{sync::Arc, time::Duration};
use tracing::{debug, info, trace};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,nectan_playground=trace"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let builder = Endpoint::builder(presets::N0);
    let endpoint = builder.bind().await.unwrap();
    let userinfo = UserInfo::new(Username::new("Simea2").unwrap());
    let (device_id, key) = gen_device_id();
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);

    tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            match m {
                AppEvent::NewConnectionRequest {
                    request_id,
                    username,
                    remote_device_id,
                    nearby,
                    respond,
                } => {
                    let _ = respond.send(UiResponse::Accept);
                    println!("New connection request from {}", username);
                }
                e => {
                    println!("#1. Got new app_event: {e:#?}");
                }
            }
        }
    });

    let tmp = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let devices = DevicesPool::new(Some(tmp)).expect("Failed to create devices pool.");
    let state1 = NectanState::build(userinfo, device_id, key, devices, tx).await;
    let prot = NectanProtocol::new(endpoint.clone(), Arc::new(state1.clone()));
    let router1 = Router::builder(endpoint.clone()).accept(ALPN, prot).spawn();
    let ep1_addr = router1.endpoint().addr();

    state1.attach_router(&router1);

    tokio::spawn(async move {
        let builder = Endpoint::builder(presets::N0);
        let endpoint = builder.bind().await.unwrap();
        let userinfo = UserInfo::new(Username::new("Simea").unwrap());
        let (device_id, key) = gen_device_id();
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);

        tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                match m {
                    e => {
                        debug!("#2. Got new app_event: {e:#?}");
                    }
                }
            }
        });

        let tmp = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let devices = DevicesPool::new(Some(tmp)).expect("Failed to create devices pool.");
        let state2 = NectanState::build(userinfo, device_id, key, devices, tx).await;
        let prot = NectanProtocol::new(endpoint.clone(), Arc::new(state2.clone()));
        let router2 = Router::builder(endpoint.clone()).accept(ALPN, prot).spawn();

        state2.attach_router(&router2);

        println!("Ep2 {}", endpoint.id().to_string());
        println!("Connecting.. ");
        println!("Connect result {:?}", connect(&state2, ep1_addr).await);
    });

    loop {
        let id = router1.endpoint().id().to_string();
        tokio::time::sleep(Duration::from_secs(1)).await;
        println!("EP1 {id}");
    }
}
