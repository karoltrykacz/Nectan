// use iroh::endpoint::Connection;
// use std::{collections::HashMap, sync::Arc};
//
// use crate::devices::DeviceId;
//
// struct ConnectionsRouter {
//     inner: Arc<HashMap<DeviceId, Connection>>,
//     tg: bool,
// }
//
// impl ConnectionsRouter {
//     pub fn start(mut self) {
//         std::thread::spawn(move || {
//             println!("Mut access to self");
//             self.tg = true;
//         });
//     }
//     pub fn jj
// }
