use ed25519_dalek::{SigningKey, rand_core::UnwrapErr};
use getrandom::{SysRng, rand_core::TryRng};
use iroh::{Endpoint, EndpointAddr, Watcher, endpoint::presets, protocol::Router};
use nectan_core::{
    messages::{NetMessage, write_message},
    protocol::{ALPN, DeviceId, NectanProtocol, NectanState},
};
use std::time::Duration;

fn gen_device_id() -> DeviceId {
    let mut csprng = UnwrapErr(SysRng);
    let key = SigningKey::generate(&mut csprng);
    key.verifying_key()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let builder = Endpoint::builder(presets::N0);
    let endpoint = builder.bind().await.unwrap();

    let device_id = gen_device_id();

    let state1 = NectanState::new(device_id).await;
    let prot = NectanProtocol::new(endpoint.clone(), state1);

    let router1 = Router::builder(endpoint).accept(ALPN, prot).spawn();
    let ep1_addr = router1.endpoint().addr();

    tokio::spawn(async move {
        let builder = Endpoint::builder(presets::N0);
        let endpoint = builder.bind().await.unwrap();

        let device_id = gen_device_id();
        let state2 = NectanState::new(device_id).await;
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
                    &NetMessage::Hello {
                        username: String::from("Zbyszek"),
                    },
                )
                .await
                .unwrap();

                // let tree = build_offer();
                // println!("Sending offer.");
                // let transfer_id = Uuid::new_v4();
                // // Send transfer offer
                // write_message(&mut tx, &Message::TransferOfferMsg { transfer_id, tree })
                //     .await
                //     .unwrap();

                // println!("Waiting for response to offer.");

                // Read response
                // let response = read_message(&mut rx).await.unwrap();
                // println!("Response to offer: {response:?}");
                //
                // // After the transfer stream msg was sent the remote device will accept the file
                //
                // // let _ = tx.finish();
                // // println!("Finished");
                //
                // // Open new stream for the transfer
                // let (mut tx, mut rx) = c.open_bi().await.unwrap();
                // let file_size = std::fs::metadata("/home/karol/Videos/Source/test.zip")
                //     .unwrap()
                //     .size();
                // let item = TransferItem {
                //     path: PathBuf::from("test.zip"),
                //     id: 0,
                //     file_size,
                //     sent_bytes: 0,
                //     err: None,
                //     is_file: true,
                // };
                // let r = send_item(transfer_id, item, &mut tx, &mut rx).await;
                // println!("{r:#?}");
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
