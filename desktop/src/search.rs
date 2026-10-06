use std::{cell::RefCell, rc::Rc};

use nucleo::{
    Config, Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use slint::{ComponentHandle, ModelRc, VecModel, Weak};

use crate::{
    AddDeviceBridge, DevicesBridge, NectanWindow, ResultItem, SearchBridge, SearchItemKind,
    SettingsBridge, Window, WindowBridge, state::ui_state,
};

trait SearchProvider {
    fn kind(&self) -> SearchItemKind;
    fn query(&self, q: &str) -> Vec<Hit>;
    fn action(&self, id: &str);
}

struct Hit {
    id: String,
    title: String,
    subtitle: String,
    tag: String,
}

struct Registry(Vec<Box<dyn SearchProvider>>);

impl Registry {
    fn search(&self, q: &str) -> Vec<ResultItem> {
        self.0
            .iter()
            .flat_map(|p| {
                let kind = p.kind();
                p.query(q).into_iter().map(move |h| ResultItem {
                    kind,
                    id: h.id.into(),
                    title: h.title.into(),
                    subtitle: h.subtitle.into(),
                    tag: h.tag.into(),
                })
            })
            .collect()
    }

    fn action(&self, kind: SearchItemKind, id: &str) {
        if let Some(p) = self.0.iter().find(|p| p.kind() == kind) {
            p.action(id);
        }
    }
}
struct Fuzzy(RefCell<Matcher>);

impl Fuzzy {
    fn new() -> Self {
        Self(RefCell::new(Matcher::new(Config::DEFAULT)))
    }

    fn score(&self, pattern: &Pattern, text: &str) -> Option<u32> {
        let mut buf = Vec::new();
        pattern.score(Utf32Str::new(text, &mut buf), &mut self.0.borrow_mut())
    }
}

fn make_pattern(q: &str) -> Pattern {
    Pattern::new(
        q,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    )
}

struct ActionProvider {
    weak: Weak<NectanWindow>,
    fuzzy: Fuzzy,
}

const ACTIONS: &[(&str, &str, &str)] = &[
    ("send-folder", "Send folder", "Pick folder to send"),
    ("send-files", "Send files", "Pick files to send"),
    ("vaults", "Vaults", "See vaults"),
    ("transfers", "Transfers", "Open transfers"),
    ("settings", "Settings", "Preferences"),
    ("add-device", "Add device", "Connect new device"),
];

impl SearchProvider for ActionProvider {
    fn kind(&self) -> SearchItemKind {
        SearchItemKind::Action
    }

    fn query(&self, q: &str) -> Vec<Hit> {
        let pattern = make_pattern(q.trim());
        let mut scored: Vec<(u32, &(&str, &str, &str))> = ACTIONS
            .iter()
            .filter_map(|a| self.fuzzy.score(&pattern, a.1).map(|s| (s, a)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));

        scored
            .into_iter()
            .map(|(_, (id, name, hint))| Hit {
                id: (*id).into(),
                title: (*name).into(),
                subtitle: (*hint).into(),
                tag: "Action".into(),
            })
            .collect()
    }

    fn action(&self, id: &str) {
        let Some(w) = self.weak.upgrade() else { return };
        match id {
            "add-device" => {
                w.global::<AddDeviceBridge>().set_open(true);
            }
            "vaults" => {
                w.global::<WindowBridge>()
                    .set_current_window(Window::Vaults);
            }
            "transfers" => {
                w.global::<WindowBridge>()
                    .set_current_window(Window::Transfers);
            }
            "send-files" => {
                let _ = slint::spawn_local(async move {
                    if let Some(files) = rfd::AsyncFileDialog::new().pick_files().await {
                        let paths: Vec<_> = files.iter().map(|f| f.path().to_owned()).collect();
                        // TODO: hand `paths` to your send logic
                        let _ = paths;
                    }
                });
            }
            "settings" => {
                w.global::<SettingsBridge>().set_open(true);
            }
            "send-folder" => {
                let _ = slint::spawn_local(async move {
                    if let Some(dir) = rfd::AsyncFileDialog::new().pick_folder().await {
                        let path = dir.path().to_owned();
                        // TODO: hand `path` to your send logic
                        let _ = path;
                    }
                });
            }
            other => {
                todo!("Unhandled action {other}.");
            }
        }
    }
}

struct DeviceProvider {
    weak: Weak<NectanWindow>,
    fuzzy: Fuzzy,
}

impl SearchProvider for DeviceProvider {
    fn kind(&self) -> SearchItemKind {
        SearchItemKind::Device
    }

    fn query(&self, q: &str) -> Vec<Hit> {
        let pattern = make_pattern(q.trim());
        let mut scored: Vec<(u32, _)> = ui_state()
            .devices()
            .items()
            .into_iter()
            .filter_map(|d| self.fuzzy.score(&pattern, d.name.as_str()).map(|s| (s, d)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));

        scored
            .into_iter()
            .map(|(_, d)| Hit {
                id: d.id.to_string(),
                title: d.name.to_string(),
                subtitle: format!("{:?}", d.status),
                tag: "Device".into(),
            })
            .collect()
    }

    fn action(&self, id: &str) {
        let Some(w) = self.weak.upgrade() else { return };
        let Some(idx) = ui_state()
            .devices()
            .items()
            .iter()
            .position(|d| d.id.as_str() == id)
        else {
            return;
        };
        w.global::<DevicesBridge>()
            .set_device_details_open_idx(idx as i32);

        w.global::<WindowBridge>()
            .set_current_window(Window::Device);
    }
}

pub fn setup_search(w: &NectanWindow) {
    let weak = w.as_weak();

    let registry = Rc::new(Registry(vec![
        Box::new(DeviceProvider {
            weak: weak.clone(),
            fuzzy: Fuzzy::new(),
        }),
        Box::new(ActionProvider {
            weak: weak.clone(),
            fuzzy: Fuzzy::new(),
        }),
    ]));

    let bridge = w.global::<SearchBridge>();

    bridge.on_query_changed({
        let reg = registry.clone();
        let weak = weak.clone();
        move |q| {
            let Some(w) = weak.upgrade() else { return };
            let items = reg.search(&q);
            w.global::<SearchBridge>()
                .set_results(ModelRc::new(VecModel::from(items)));
        }
    });

    bridge.on_action({
        let reg = registry.clone();
        move |kind, id| reg.action(kind, &id)
    });
}
