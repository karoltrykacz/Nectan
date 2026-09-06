use futures::StreamExt;
use nectan_core::{
    path_tree::CompressedPathTree,
    protocol::{ALPN, Message, NectanProtocol, read_message, write_message},
    walker::Walker,
};
use std::{path::PathBuf, time::Duration};
use uuid::Uuid;

use iroh::{Endpoint, EndpointAddr, Watcher, endpoint::presets, protocol::Router};

#[tokio::main]
async fn main() {
    let builder = Endpoint::builder(presets::N0);
    let endpoint = builder.bind().await.unwrap();
    let prot = NectanProtocol::new(endpoint.clone());
    // let watcher = endpoint.watch_addr();
    // tokio::spawn(async move {
    //     let mut updates = watcher.stream();
    //     while let Some(addr) = updates.next().await {
    //         tracing::trace!("EP1 changed {addr:#?}");
    //     }
    // });
    let router1 = Router::builder(endpoint).accept(ALPN, prot).spawn();
    let ep1_addr = router1.endpoint().addr();

    tokio::spawn(async move {
        let builder = Endpoint::builder(presets::N0);
        let endpoint = builder.bind().await.unwrap();
        let prot = NectanProtocol::new(endpoint.clone());
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
                // Send transfer offer
                write_message(
                    &mut tx,
                    &Message::TransferOffer {
                        id: Uuid::new_v4(),
                        tree,
                    },
                )
                .await
                .unwrap();

                // Read response
                let response = read_message(&mut rx).await.unwrap();
                println!("Response to offer: {response:?}");

                let _ = tx.finish();
                println!("Finished");
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
