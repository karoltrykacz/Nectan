use crate::{vault::Vault, walker::Walker};
use std::path::{Path, PathBuf};

mod vault;
mod walker;

pub const MB: f32 = 1024.0 * 1024.0;

fn main() {
    tracing_subscriber::fmt::init();

    let root = PathBuf::from("/media/karol/kox");
    let vault = Vault::new(root);
}
