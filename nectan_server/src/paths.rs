use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub fn resolve(path: &Path) -> Result<PathBuf> {
    resolve_with_home(path, dirs::home_dir().as_deref())
}

fn resolve_with_home(path: &Path, home: Option<&Path>) -> Result<PathBuf> {
    let path = match path.strip_prefix("~") {
        Ok(rest) => home
            .context("cannot determine daemon home directory")?
            .join(rest),
        Err(_) => path.to_owned(),
    };
    Ok(std::path::absolute(path)?)
}

pub fn app_data() -> PathBuf {
    dirs::data_dir().unwrap().join("nectan_server")
}
