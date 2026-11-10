use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap, error::Error, hash::Hash, sync::RwLock,
};
use utils::storage_utils::{DataWriter, JsonSerializer};

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Appearance {
    pub theme: String,
    pub font: String,
    pub font_scale: f32,
    pub edges_shape: f32,
    pub round_window_edges: bool,
    pub native_topbar: bool,
    pub topbar_always_on: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            font: "Rubik".into(),
            font_scale: 1.0,
            edges_shape: 2.0,
            round_window_edges: true,
            native_topbar: false,
            topbar_always_on: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct StoredShortcut {
    pub keys: Vec<String>,
    pub alt_keys: Vec<String>,
}

impl Default for InnerCommonStorage {
    fn default() -> Self {
        InnerCommonStorage {
            language: "en".into(),
            appearance: Appearance::default(),
            shortcuts: HashMap::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct InnerCommonStorage {
    pub language: String,
    pub appearance: Appearance,
    pub shortcuts: HashMap<String, StoredShortcut>,
}

pub struct CommonStorage {
    inner: std::sync::RwLock<InnerCommonStorage>,
    writer: DataWriter<JsonSerializer>,
}

impl CommonStorage {
    pub fn new() -> Self {
        let writer = DataWriter::<JsonSerializer>::new(
            dirs::data_dir().unwrap().join("Nectan"),
            "appearance",
        )
        .unwrap();

        let initial =
            writer.load::<InnerCommonStorage>().unwrap_or_default();
        let inner = RwLock::new(initial);
        CommonStorage { inner, writer }
    }

    pub fn read<R>(
        &self,
        f: impl FnOnce(&InnerCommonStorage) -> R,
    ) -> R {
        f(&self.inner.read().unwrap())
    }

    pub fn update(
        &self,
        f: impl FnOnce(&mut InnerCommonStorage),
    ) -> Result<(), Box<dyn Error>> {
        let mut guard = self.inner.write().unwrap();
        let mut next = guard.clone();
        f(&mut next);
        self.writer.write(&next)?;
        *guard = next;
        Ok(())
    }

    pub fn get(&self) -> InnerCommonStorage {
        self.inner.read().unwrap().clone()
    }

    pub fn set(&self, new: InnerCommonStorage) {
        let _ = self.writer.write(&new);
        *self.inner.write().unwrap() = new;
    }

    pub fn set_appearance(&self, new: Appearance) {
        self.update(|s| s.appearance = new);
    }

    pub fn set_lang(&self, new: String) {
        self.update(|s| s.language = new);
    }

    pub fn shortcuts(&self) -> HashMap<String, StoredShortcut> {
        self.read(|s| s.shortcuts.clone())
    }
    pub fn lang(&self) -> String {
        self.read(|s| s.language.clone())
    }
}
