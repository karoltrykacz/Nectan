use crate::access::Access;
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};
use tokio::sync::RwLock;
use tracing::{error, info};
use uuid::Uuid;

pub struct Vault {
    id: Uuid,
    name: String,
    root: PathBuf,
    access: Access,
}

impl fmt::Debug for Vault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vault")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
enum VaultCreationError {
    InvalidName,
    AlreadyExists,
    PermissionDenied,
    ReadOnlyFilesystem,
    StorageFull,
    FsError,
}

impl Vault {
    /// Creates a new vault in `<root>/<name>`.
    pub fn create(root: impl AsRef<Path>, name: String) -> Result<Self, VaultCreationError> {
        let root = root.as_ref();

        // Validate vault name
        if !is_valid_vault_name(&name) {
            return Err(VaultCreationError::InvalidName);
        }

        let vault_path = root.join(&name);
        if let Err(e) = std::fs::create_dir(&vault_path) {
            error!(
                "Failed to create vault directory: e={e:?}, path={}",
                root.display()
            );
            return Err(map_io_error(e.kind()));
        }

        let id = Uuid::new_v4();
        let access = Access::load(&vault_path).map_err(|_e| VaultCreationError::FsError)?;

        Ok(Self {
            id,
            name,
            root: vault_path,
            access,
        })
    }
}

fn is_valid_vault_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn map_io_error(kind: ErrorKind) -> VaultCreationError {
    match kind {
        ErrorKind::PermissionDenied => VaultCreationError::PermissionDenied,
        ErrorKind::AlreadyExists => VaultCreationError::AlreadyExists,
        ErrorKind::ReadOnlyFilesystem => VaultCreationError::ReadOnlyFilesystem,
        ErrorKind::StorageFull => VaultCreationError::StorageFull,
        _ => VaultCreationError::FsError,
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum VaultCmd {
    /// File operations
    /// Receive file
    Get(PathBuf),
    /// Put file in vault
    // Put(TransferItemHeader),
    MakeDir(PathBuf),
    /// Delete file. Moves file to trash or deletes permanently if trash is disabled.
    Delete(PathBuf),
    /// Min(vault space limit, avaliable disk space)
    AvaliableSpace,
    /// File system metadata
    Metadata(PathBuf),
    /// Autocomplete
    Complete(PathBuf),
    /// Information about vault
    VaultInfo,
    /// Get the generated preview of file
    Preview,
}

pub struct Vaults {
    inner: RwLock<Vec<Vaults>>,
}

impl fmt::Debug for Vaults {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.inner.try_read() {
            Ok(vaults) => f.debug_list().entries(vaults.iter()).finish(),
            Err(_) => f.write_str("<locked>"),
        }
    }
}

impl Vaults {
    pub fn load() -> anyhow::Result<Self> {
        let vaults = Vec::new();
        info!("Loaded vaults [{vaults:#?}]");
        Ok(Vaults {
            inner: RwLock::new(vaults),
        })
    }
}
