use std::path::{Path, PathBuf};

pub struct Explorer {
    current: PathBuf,
}

impl Explorer {
    pub fn new() -> Self {
        Explorer {
            current: dirs::home_dir().unwrap(),
        }
    }

    pub fn open(p: impl AsRef<Path>) {}
}
