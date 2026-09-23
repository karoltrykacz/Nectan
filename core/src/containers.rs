// pub struct Remote;
// pub struct Local;
//
// pub trait ContainerKind {
//     type Extra;
// }
//
// pub struct LocalExtra {
//     pub root_path: PathBuf,
// }
//
// pub struct RemoteExtra {
//     pub remote_device_id: DeviceId,
// }
//
// impl ContainerKind for Local {
//     type Extra = LocalExtra;
// }
//
// impl ContainerKind for Remote {
//     type Extra = RemoteExtra;
// }
//
// pub struct Container<T: ContainerKind> {
//     id: Uuid,
//     name: Username,
//     extra: T::Extra,
// }
//
// pub enum AnyContainer {
//     Local(Container<Local>),
//     Remote(Container<Remote>),
// }
//
// impl<T: ContainerKind> Container<T> {
//     pub fn get_hash(&self) {}
// }
//
// impl Container<Local> {
//     pub fn new(name: Username, path: PathBuf) -> Self {
//         Container {
//             id: Uuid::new_v4(),
//             name,
//             extra: LocalExtra { root_path: path },
//         }
//     }
//     pub fn get_file() {}
// }
//
// impl Container<Remote> {
//     pub fn get_file(&self, stream: StreamPair) {
//         // Send request to remote device
//         // Where from get the connection
//     }
// }
//
// #[derive(Clone, Debug, Serialize, Deserialize)]
// pub enum ContainerCommand {
//     GetFile { id: u32 },
//     // PutFile { id: u32 },
// }
//
// struct DumbContainer {
//     id: Uuid,
//     root_path: PathBuf,
//     whitelist: Vec<DeviceId>,
//     // compression: bool,
//     tx: tokio::sync::mpsc::Sender<ContainerCommand>,
// }
//
// const DUMB_ITEMS: TableDefinition<u32, TransferItem> = TableDefinition::new("items");
//
// impl DumbContainer {
//     pub async fn handle_stream(&self, sender: DeviceId, mut stream: StreamPair) -> Result<()> {
//         if !self.whitelist.contains(&sender) {
//             bail!("Sender not whitelisted.")
//         }
//
//         let cmd: ContainerCommand = stream.read().await?;
//         match cmd {
//             ContainerCommand::GetFile { id } => self.handle_get_file(id, stream).await,
//         }
//         Ok(())
//     }
//
//     // File ids not feasible for long lasting containers (need to keep track of the ids)
//     // File names hard to keep track of permitted files (for send files option)
//     async fn handle_get_file(&self, id: u32, stream: StreamPair) {
//         if let Some(item) = self.get_item(id) {
//             // ????
//             send_item(item, stream).await;
//         } else {
//             // stream.write(msg)
//         };
//     }
//
//     // pub async fn run_controller(&self, mut rx: tokio::sync::mpsc::Receiver<ContainerCommand>) {
//     //     // Accept requests from the devices
//     //     while let Some(cmd) = rx.recv().await {
//     //         match cmd {
//     //             GetFile { id } => {
//     //                 info!("Sending file [{id}]");
//     //             }
//     //         }
//     //     }
//     // }
// }
//
// struct ContainerRequest {
//     id: Uuid,
//     sender: DeviceId,
//     stream: StreamPair,
// }
//
// struct DumbContainers {
//     containers: HashMap<Uuid, DumbContainer>,
//     tx: tokio::sync::mpsc::Sender<ContainerRequest>,
// }
//
// impl DumbContainers {
//     pub async fn run_controller(&mut self, mut rx: tokio::sync::mpsc::Receiver<ContainerRequest>) {
//         while let Some(r) = rx.recv().await {
//             if let Some(c) = self.containers.get(&r.id) {
//                 let _ = c.handle_stream(r.sender, r.stream).await;
//             }
//         }
//     }
// }
//
// #[derive(Clone)]
// pub struct Containers {
//     containers: Arc<HashMap<Uuid, AnyContainer>>,
// }
//
// impl Containers {
//     pub fn new() -> Self {
//         let initial = HashMap::new();
//
//         Containers {
//             containers: Arc::new(initial),
//         }
//     }
//     pub fn create_container() {}
//
//     /// Adds remote container
//     pub fn add_remote(owner: DeviceId, id: Uuid, name: Username) {
//         let c = Container::<Remote> {
//             id,
//             name,
//             extra: RemoteExtra {
//                 remote_device_id: owner,
//             },
//         };
//     }
// }
//
// const CONTAINERS: TableDefinition<u128, &[u8]> = TableDefinition::new("transfers");
