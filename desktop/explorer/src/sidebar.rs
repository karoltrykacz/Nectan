use crate::{
    EssentialTileData, ExplorerBridge, NectanWindow, SidebarBridge,
    WindowBridge, state::ui_state,
};
use slint::{
    ComponentHandle, Image, Model, ModelNotify, ModelRc, ModelTracker,
    SharedString, ToSharedString, VecModel,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

#[derive(Clone, Debug)]
pub enum Builtin {
    Documents,
    Images,
    Videos,
    Trash,
    Music,
    Downloads,
    Recent,
    Home,
    Drives,
}
impl From<crate::Builtin> for Builtin {
    fn from(b: crate::Builtin) -> Self {
        match b {
            crate::Builtin::Recent => Builtin::Recent,
            crate::Builtin::Documents => Builtin::Documents,
            crate::Builtin::Pictures => Builtin::Images,
            crate::Builtin::Videos => Builtin::Videos,
            crate::Builtin::Downloads => Builtin::Downloads,
            crate::Builtin::Home => Builtin::Home,
            crate::Builtin::Music => Builtin::Music,
            crate::Builtin::Trash => Builtin::Trash,
            crate::Builtin::Drives => Builtin::Drives,
        }
    }
}

#[derive(Clone, Debug)]
pub enum TileAction {
    Builtin(Builtin),
}

impl TileAction {
    pub fn execute(&self, w: &NectanWindow) {
        let dir = match self {
            TileAction::Builtin(b) => match b {
                Builtin::Recent => dirs::home_dir(),
                Builtin::Home => dirs::home_dir(),
                Builtin::Documents => dirs::document_dir(),
                Builtin::Images => dirs::picture_dir(),
                Builtin::Videos => dirs::video_dir(),
                Builtin::Music => dirs::audio_dir(),
                Builtin::Downloads => dirs::download_dir(),
                Builtin::Drives => dirs::download_dir(),
                Builtin::Trash => dirs::home_dir(),
            },
        }
        .unwrap();

        w.global::<WindowBridge>()
            .set_current_window(crate::Window::Vaults);

        ui_state().explorer().open_dir(w.as_weak(), dir);
    }
}

#[derive(Default)]
struct ActionsRegistry {
    next: i32,
    actions: HashMap<i32, TileAction>,
}

impl ActionsRegistry {
    fn add(&mut self, action: TileAction) -> i32 {
        let id = self.next;
        self.next += 1;
        self.actions.insert(id, action);
        id
    }

    fn remove(&mut self, id: i32) {
        self.actions.remove(&id);
    }

    fn get(&self, id: i32) -> Option<TileAction> {
        self.actions.get(&id).cloned()
    }
}

pub struct EssentialsModel {
    tiles: RefCell<Vec<EssentialTileData>>,
    actions: RefCell<ActionsRegistry>,
    notify: ModelNotify,
}

impl EssentialsModel {
    pub fn new() -> Self {
        let load = |svg: &'static [u8]| Image::load_from_svg_data(svg).unwrap();

        // Order here = initial slot order.
        let defaults: Vec<(Builtin, Image, &'static str)> = vec![
            (
                Builtin::Recent,
                load(include_bytes!("../ui/icons/clock.svg")),
                "",
            ),
            (
                Builtin::Home,
                load(include_bytes!("../ui/icons/home.svg")),
                "",
            ),
            (
                Builtin::Documents,
                load(include_bytes!("../ui/icons/file-nofill.svg")),
                "",
            ),
            (
                Builtin::Images,
                load(include_bytes!("../ui/icons/image.svg")),
                "",
            ),
            (
                Builtin::Videos,
                load(include_bytes!("../ui/icons/film.svg")),
                "",
            ),
            (
                Builtin::Music,
                load(include_bytes!("../ui/icons/music.svg")),
                "",
            ),
            (
                Builtin::Downloads,
                load(include_bytes!("../ui/icons/download.svg")),
                "",
            ),
            (
                Builtin::Drives,
                load(include_bytes!("../ui/icons/hard-drive.svg")),
                "",
            ),
            (
                Builtin::Trash,
                load(include_bytes!("../ui/icons/trash.svg")),
                "",
            ),
        ];

        let mut actions = ActionsRegistry::default();
        let tiles: Vec<_> = defaults
            .into_iter()
            .enumerate()
            .map(|(slot, (builtin, icon, emoji))| {
                let id = actions.add(TileAction::Builtin(builtin));
                EssentialTileData {
                    id,
                    slot: slot as i32,
                    emoji: emoji.into(),
                    icon,
                }
            })
            .collect();

        Self {
            tiles: RefCell::new(tiles),
            actions: RefCell::new(actions),
            notify: ModelNotify::default(),
        }
    }

    /// Look up the action for a clicked tile id.
    pub fn action_for(&self, id: i32) -> Option<TileAction> {
        self.actions.borrow().get(id)
    }

    pub fn id_at_slot(&self, slot: i32) -> i32 {
        self.tiles
            .borrow()
            .iter()
            .find(|t| t.slot == slot)
            .map(|t| t.id)
            .unwrap_or(-1)
    }

    pub fn reorder(&self, id: i32, target: i32) {
        let mut tiles = self.tiles.borrow_mut();

        let Some(a) = tiles.iter().position(|t| t.id == id) else {
            return;
        };
        let Some(b) = tiles.iter().position(|t| t.slot == target) else {
            if tiles[a].slot != target {
                tiles[a].slot = target;
                drop(tiles);
                self.notify.row_changed(a);
            }
            return;
        };

        if a == b {
            return;
        }

        let slot_a = tiles[a].slot;
        tiles[a].slot = tiles[b].slot;
        tiles[b].slot = slot_a;

        drop(tiles);
        self.notify.row_changed(a);
        self.notify.row_changed(b);
    }
}

impl Model for EssentialsModel {
    type Data = EssentialTileData;

    fn row_count(&self) -> usize {
        self.tiles.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        self.tiles.borrow().get(row).cloned()
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

pub fn handle_sidebar(w: &NectanWindow) {
    let b = w.global::<SidebarBridge>();
    let model = Rc::new(EssentialsModel::new());

    b.set_essentials_data(ModelRc::from(model.clone()));

    let m = model.clone();
    b.on_id_at_slot(move |slot| m.id_at_slot(slot));

    let m = model.clone();
    b.on_reorder(move |id, target| m.reorder(id, target));

    let m = model.clone();
    let weak = w.as_weak();
    b.on_clicked(move |id| {
        let Some(w) = weak.upgrade() else {
            return;
        };
        if let Some(action) = m.action_for(id) {
            action.execute(&w);
        }
    });
}
