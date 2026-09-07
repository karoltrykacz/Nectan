#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use anyhow::Result;
use nectan_core::path_tree::PathTree;
use nectan_core::protocol::{Message, NectanState, build_offer};
use slint::winit_030::WinitWindowAccessor;
use slint::{ModelRc, ToSharedString, VecModel, Weak};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    let w = NectanWindow::new()?;

    let state = NectanState::new();
    start_event_listener(&w, state.clone());

    handle_window_controls(&w);

    let tree = build_offer();

    state
        .sender()
        .send(Message::TransferOffer {
            transfer_id: Uuid::default(),
            tree,
        })
        .unwrap();

    w.run()
}

pub fn handle_window_controls(w: &NectanWindow) {
    let w_weak = w.as_weak();
    let bridge = w.global::<WindowBridge>();

    // Titlebar drag
    bridge.on_drag_window(move || {
        if let Some(w) = w_weak.upgrade() {
            w.window().with_winit_window(|winit_win| {
                let _ = winit_win.drag_window();
            });
        }
    });
    let w_weak = w.as_weak();
    bridge.on_minimize_window(move || {
        if let Some(w) = w_weak.upgrade() {
            w.window().set_minimized(true);
        }
    });

    let w_weak = w.as_weak();
    bridge.on_maximize_window(move || {
        if let Some(w) = w_weak.upgrade() {
            let win = w.window();
            win.set_maximized(!win.is_maximized());
        }
    });

    let w_weak = w.as_weak();
    bridge.on_close_window(move || {
        if let Some(w) = w_weak.upgrade() {
            let _ = w.hide();
        }
    });
}

pub fn start_event_listener(w: &NectanWindow, state: NectanState) {
    let mut event_rx = state.subscribe_to_events();
    let w = w.as_weak();
    tokio::spawn(async move {
        while let Ok(msg) = event_rx.recv().await {
            tracing::info!("New app event: {msg:#?}");
            match msg {
                Message::TransferOffer { transfer_id, tree } => {
                    show_transfer_offer(&w, transfer_id, tree);
                }
                _ => {
                    panic!("Invalid app event. {msg:#?}")
                }
            }
        }
    });
}

pub fn show_transfer_offer(w: &Weak<NectanWindow>, _transfer_id: Uuid, tree: PathTree) {
    let _ = w.upgrade_in_event_loop(move |w| {
        let b = w.global::<IncomingTransferOfferBridge>();
        b.set_is_open(true);

        let files: Vec<_> = tree
            .to_vec()
            .iter()
            .map(|(path, is_file)| TransferOfferItem {
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_shared_string(),
                size_text: "12 GB".into(),
            })
            .collect();
        let transfer_offer = IncomingTransferOffer {
            total_entries: 123,
            total_size: "12 GB".into(),
            from: "Zbyszek".into(),
            transfer_name: "Siema".into(),
            items: ModelRc::new(VecModel::from(files)),
        };
        b.set_offer(transfer_offer);
    });
}

pub fn handle_incoming_transfer_offer(w: &NectanWindow) {
    let b = w.global::<IncomingTransferOfferBridge>();
    let weak = w.as_weak();
    b.on_accept(move || {
        tracing::info!("Accepted");
        if let Some(w) = weak.upgrade() {
            let b = w.global::<IncomingTransferOfferBridge>();
            b.set_is_open(false);

            tokio::spawn(async move {
                app_state()
                    .transfer_offer()
                    .respond_to_offer(transfer_id, true)
                    .await;
            });
        }
    });

    let weak = w.as_weak();
    b.on_reject(move |transfer_id| {
        tracing::info!("Rejected");
        if let Some(w) = weak.upgrade() {
            let b = w.global::<IncomingTransferOfferBridge>();
            b.set_is_open(false);
            let Ok(transfer_id) = Uuid::parse_str(&transfer_id) else {
                return;
            };
            tokio::spawn(async move {
                app_state()
                    .transfer_offer()
                    .respond_to_offer(transfer_id, false)
                    .await;
            });
        }
    });

    let weak = w.as_weak();
    b.on_toggle_node(move |id, path| {
        let Some(w) = weak.upgrade() else { return };

        let bridge = w.global::<IncomingTransferOfferBridge>();
        let nodes = bridge.get_nodes();
        let mut nodes: Vec<TreeNode> = nodes.iter().map(|node| node.clone()).collect();
        if nodes[id as usize].is_file {
            return;
        }
        let parent_depth = nodes[id as usize].depth;

        if nodes[id as usize].expanded {
            nodes[id as usize].expanded = false;
            // Close the nodes
            let mut end = id as usize + 1;
            while end < nodes.len() && nodes[end].depth > parent_depth {
                end += 1;
            }
            nodes.drain(id as usize + 1..end);

            let model = VecModel::from(nodes);
            bridge.set_nodes(ModelRc::from(Rc::new(model)));
            return;
        }

        nodes[id as usize].expanded = true;

        let offer = app_state().transfer_offer();
        let weak = w.as_weak();

        // Read from the tree the sender sent us
        tokio::spawn(async move {
            let depth = parent_depth + 1;
            let path = Path::new(&path);
            let lock = offer.read().await;
            let info = lock.as_ref().unwrap();

            let children: Vec<TreeNode> = info
                .info
                .tree
                .children_of(path)
                .iter()
                .map(|(p, is_file)| TreeNode {
                    depth,
                    expanded: false,
                    is_file: *is_file,
                    filename: p.file_name().unwrap().to_string_lossy().to_string().into(),
                    path: p.to_string_lossy().to_string().into(),
                })
                .collect();
            nodes.splice((id + 1) as usize..(id + 1) as usize, children);

            let _ = weak.upgrade_in_event_loop(move |w| {
                let bridge = w.global::<IncomingTransferOfferBridge>();
                let model = VecModel::from(nodes);
                bridge.set_nodes(ModelRc::from(Rc::new(model)));
            });
        });
    });
}

pub struct TransferOfferInner {
    pub info: TransferOfferInfo<PathTree>,
    sender_id: DeviceId,
    sender_name: Username,
    respond: tokio::sync::mpsc::Sender<MessagePayload>,
}

#[derive(Clone)]
pub struct TransferOffer {
    inner: Arc<tokio::sync::Mutex<Option<TransferOfferInner>>>,
    window: Weak<MainWindow>,
}

/// Only one connection request is allowed at a time
/// If some offer is pending, all incoming will be discarded
impl TransferOffer {
    fn new(window: Weak<MainWindow>) -> Self {
        TransferOffer {
            inner: Arc::new(tokio::sync::Mutex::new(None)),
            window,
        }
    }

    pub async fn take(&self) -> Option<TransferOfferInner> {
        self.inner.lock().await.take()
    }

    pub async fn read(&self) -> tokio::sync::MutexGuard<'_, Option<TransferOfferInner>> {
        self.inner.lock().await
    }
    pub async fn add(
        &self,
        info: TransferOfferInfo<PathTree>,
        sender_name: Username,
        sender_id: DeviceId,
        respond: tokio::sync::mpsc::Sender<MessagePayload>,
    ) {
        let mut lock = self.inner.lock().await;
        match *lock {
            Some(_) => {
                // Some offer is already shown to the user
                // Automatically reject the offer
                let _ = respond.send(MessagePayload::RejectTransfer).await;
            }
            None => {
                // Prepare and show user the offer
                self.show(
                    sender_name.to_string(),
                    info.transfer_name.clone(),
                    info.entries_num,
                    info.total_size,
                    info.transfer_id,
                    info.tree.get_depth_0(),
                );
                *lock = Some(TransferOfferInner {
                    sender_id,
                    sender_name,
                    info,
                    respond,
                });
            }
        }
    }
    pub async fn respond_to_offer(&self, transfer_id: Uuid, accept: bool) {
        let response = match accept {
            true => MessagePayload::AcceptTransfer,
            false => MessagePayload::RejectTransfer,
        };
        let mut lock = self.inner.lock().await;
        // Consume the offer
        if let Some(offer) = lock.take() {
            let _ = offer.respond.send(response).await;
        }
    }

    fn show(
        &self,
        from: String,
        name: String,
        total_entries: u64,
        total_size: u64,
        id: Uuid,
        mut depth0: Vec<(PathBuf, bool)>,
    ) {
        let nodes: Vec<TreeNode> = depth0
            .drain(..)
            .map(|(path, is_file)| TreeNode {
                depth: 0,
                expanded: false,
                is_file,
                filename: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
                    .into(),
                path: path.to_string_lossy().to_string().into(),
            })
            .collect();

        let _ = self.window.upgrade_in_event_loop(move |w| {
            let b = w.global::<IncomingTransferOfferBridge>();
            let offer = IncomingTransferOffer {
                from: from.into(),
                transfer_id: id.to_string().into(),
                total_size: format_bytes(total_size).into(),
                transfer_name: name.into(),
                total_entries: total_entries as i32,
            };
            b.set_nodes(ModelRc::new(VecModel::from(nodes)));
            b.set_offer(offer);
            b.set_is_open(true);
        });
    }
}
