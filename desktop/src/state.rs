pub struct TransferOfferInner {
    pub info: TransferOfferInfo<PathTree>,
    sender_id: DeviceId,
    sender_name: Username,
    respond: tokio::sync::mpsc::Sender<MessagePayload>,
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
