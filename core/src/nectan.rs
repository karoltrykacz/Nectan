use futures::StreamExt;
use iroh::{Endpoint, EndpointAddr, Watcher, endpoint::presets, protocol::Router};
use nectan_core::{
    path_tree::CompressedPathTree,
    protocol::{ALPN, Message, NectanProtocol, NectanState, read_message, write_message},
    transfers::{TransferItem, send_item},
    walker::Walker,
};
use std::{fs::metadata, os::unix::fs::MetadataExt, path::PathBuf, time::Duration};
use uuid::Uuid;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let builder = Endpoint::builder(presets::N0);
    let endpoint = builder.bind().await.unwrap();

    let state1 = NectanState::new();
    let prot = NectanProtocol::new(endpoint.clone(), state1);

    let router1 = Router::builder(endpoint).accept(ALPN, prot).spawn();
    let ep1_addr = router1.endpoint().addr();

    tokio::spawn(async move {
        let builder = Endpoint::builder(presets::N0);
        let endpoint = builder.bind().await.unwrap();
        let state2 = NectanState::new();
        let prot = NectanProtocol::new(endpoint.clone(), state2);
        println!("Ep2 {}", endpoint.id().to_string());
        let router = Router::builder(endpoint.clone()).accept(ALPN, prot).spawn();

        println!("Connecting.. ");
        let r = endpoint.connect(ep1_addr, ALPN).await;
        match r {
            Ok(c) => {
                println!("Connected");
                let (mut tx, mut rx) = c.open_bi().await.unwrap();
                println!("Opened bi");
                write_message(
                    &mut tx,
                    &Message::Hello {
                        username: String::from("Zbyszek"),
                    },
                )
                .await
                .unwrap();

                let tree = build_offer();
                println!("Sending offer.");
                let transfer_id = Uuid::new_v4();
                // Send transfer offer
                write_message(&mut tx, &Message::TransferOffer { transfer_id, tree })
                    .await
                    .unwrap();

                println!("Waiting for response to offer.");

                // Read response
                let response = read_message(&mut rx).await.unwrap();
                println!("Response to offer: {response:?}");

                // After the transfer stream msg was sent the remote device will accept the file

                // let _ = tx.finish();
                // println!("Finished");

                // Open new stream for the transfer
                let (mut tx, mut rx) = c.open_bi().await.unwrap();
                let file_size = std::fs::metadata("/home/karol/Videos/Source/test.zip")
                    .unwrap()
                    .size();
                let item = TransferItem {
                    path: PathBuf::from("test.zip"),
                    id: 0,
                    file_size,
                    sent_bytes: 0,
                    err: None,
                    is_file: true,
                };
                let r = send_item(transfer_id, item, &mut tx, &mut rx).await;
                println!("{r:#?}");
            }
            Err(e) => {
                println!("Failed to connect {e:#?}");
            }
        }
    });

    loop {
        let id = router1.endpoint().id().to_string();
        tokio::time::sleep(Duration::from_secs(1)).await;
        println!("EP1 {id}");
    }
}
pub fn build_offer() -> CompressedPathTree {
    let paths = vec![PathBuf::from("/home/karol/Documents")];
    let walker = Walker::new(paths, true, true);
    walker.walk().join().unwrap();
    walker.tree.lock().unwrap().take().unwrap()
}
