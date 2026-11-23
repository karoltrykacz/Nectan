use anyhow::Context;
use anyhow::Result;
use iroh::EndpointId;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::watch;
use tracing::info;

#[derive(Debug, Clone)]
pub struct Access {
    path: PathBuf,
    mutation: Arc<Mutex<()>>,
    allowed: watch::Sender<BTreeSet<EndpointId>>,
}

impl Access {
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        let path = root.join("control-allowed.json");
        let allowed: BTreeSet<EndpointId> = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("invalid control allowlist")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
            Err(error) => return Err(error.into()),
        };

        info!("Loaded allowed peers for {}. {allowed:#?}", root.display());

        let (allowed, _) = watch::channel(allowed);

        Ok(Self {
            path,
            mutation: Arc::new(Mutex::new(())),
            allowed,
        })
    }

    pub fn list(&self) -> Vec<EndpointId> {
        self.allowed.borrow().iter().copied().collect()
    }

    pub fn update(&self, endpoint: EndpointId, allow: bool) -> Result<()> {
        let _guard = self
            .mutation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut allowed = self.allowed.borrow().clone();
        if allow {
            allowed.insert(endpoint);
        } else {
            allowed.remove(&endpoint);
        }
        let temporary = self
            .path
            .with_extension(format!("{}.tmp", rand::random::<u64>()));
        let result = (|| -> Result<()> {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(&serde_json::to_vec_pretty(&allowed)?)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result?;
        self.allowed.send_replace(allowed);
        Ok(())
    }
}
