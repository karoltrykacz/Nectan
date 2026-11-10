use frizbee::CaseMatching;
use frizbee::Config;
use frizbee::Matcher;
use gio::AppInfo;
use gio::prelude::*;
use slint::ComponentHandle;
use slint::Image;
use slint::Model;
use slint::ModelRc;
use slint::ToSharedString;
use slint::VecModel;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::path::PathBuf;
use std::rc::Rc;

use crate::{App, OpenWithApp, OpenWithBridge};

struct State {
    items: RefCell<Vec<OpenWithApp>>,
    view: Rc<VecModel<OpenWithApp>>,
    needle: RefCell<String>,
    targets: RefCell<Vec<PathBuf>>,
}

thread_local! {
    static STATE: State = State {
        items: RefCell::new(Vec::new()),
        view: Rc::new(VecModel::default()),
        needle: RefCell::new(String::new()),
        targets: RefCell::new(Vec::new()),
    };
}

struct AppData {
    id: String,
    name: String,
    icon_path: Option<PathBuf>,
}

pub fn set_open_with_targets(paths: Vec<PathBuf>) {
    STATE.with(|s| *s.targets.borrow_mut() = paths);
}

pub fn discover_apps() -> Vec<AppInfo> {
    let mut apps: Vec<AppInfo> = AppInfo::all()
        .into_iter()
        .filter(|a| a.should_show())
        .collect();
    apps.sort_by_key(|app| app.display_name().to_lowercase());
    apps
}

pub fn gio_icon_to_path(
    icon: &gio::Icon,
    target_size: u16,
) -> Option<PathBuf> {
    let mut candidate_paths: Vec<PathBuf> = Vec::new();

    if let Some(file_icon) = icon.downcast_ref::<gio::FileIcon>() {
        if let Some(path) = file_icon.file().path() {
            candidate_paths.push(path);
        }
    } else if let Some(themed_icon) =
        icon.downcast_ref::<gio::ThemedIcon>()
    {
        let home = std::env::var("HOME").unwrap_or_default();
        let sz = target_size.to_string();

        for name in themed_icon.names() {
            let name_str = name.to_string();
            let path = PathBuf::from(&name_str);

            if path.is_absolute() {
                candidate_paths.push(path);
            } else {
                candidate_paths.extend([
                    PathBuf::from(format!("/usr/share/icons/hicolor/{sz}x{sz}/apps/{name_str}.png")),
                    PathBuf::from(format!("/usr/share/icons/hicolor/scalable/apps/{name_str}.svg")),
                    PathBuf::from(format!("/usr/share/pixmaps/{name_str}.png")),
                    PathBuf::from(format!("/usr/share/pixmaps/{name_str}.xpm")),
                    PathBuf::from(format!("/usr/share/icons/hicolor/48x48/apps/{name_str}.png")),
                    PathBuf::from(format!("/usr/share/icons/hicolor/128x128/apps/{name_str}.png")),
                    PathBuf::from(format!("{home}/.local/share/icons/hicolor/{sz}x{sz}/apps/{name_str}.png")),
                    PathBuf::from(format!("{home}/.local/share/icons/hicolor/scalable/apps/{name_str}.svg")),
                    PathBuf::from(format!("{home}/.local/share/icons/{name_str}.png")),
                    PathBuf::from(format!("/var/lib/flatpak/exports/share/icons/hicolor/{sz}x{sz}/apps/{name_str}.png")),
                    PathBuf::from(format!("/var/lib/flatpak/exports/share/icons/hicolor/scalable/apps/{name_str}.svg")),
                    PathBuf::from(format!("{home}/.local/share/flatpak/exports/share/icons/hicolor/{sz}x{sz}/apps/{name_str}.png")),
                    PathBuf::from(format!("/var/lib/snapd/desktop/icons/{name_str}.png")),
                ]);
            }
        }
    }

    candidate_paths.into_iter().find(|p| p.exists())
}

fn filter() {
    STATE.with(|s| {
        let needle = s.needle.borrow();
        let items = s.items.borrow();

        if needle.is_empty() {
            s.view.set_vec(items.clone());
            return;
        }

        let haystacks: Vec<&str> =
            items.iter().map(|a| a.name.as_str()).collect();

        let config = Config {
            casing: CaseMatching::Smart,
            ..Config::default()
        };

        let mut matcher = Matcher::new(needle.as_str(), &config);
        let mut matches = matcher.match_list_parallel(&haystacks, 0);
        matches.sort_by_key(|m| Reverse(m.score));

        let hits: Vec<OpenWithApp> = matches
            .into_iter()
            .map(|m| items[m.index as usize].clone())
            .collect();

        s.view.set_vec(hits);
    });
}

fn launch_app(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("app has no desktop id".into());
    }

    let app = AppInfo::all()
        .into_iter()
        .find(|a| a.id().is_some_and(|i| i.as_str() == id))
        .ok_or_else(|| format!("desktop entry not found: {id}"))?;

    let files: Vec<gio::File> = STATE.with(|s| {
        s.targets.borrow().iter().map(gio::File::for_path).collect()
    });

    app.launch(&files, None::<&gio::AppLaunchContext>)
        .map_err(|e| e.to_string())
}

pub fn handle_open_with(w: &App) {
    let b = w.global::<OpenWithBridge>();
    let weak = w.as_weak();

    b.set_apps(ModelRc::from(STATE.with(|s| s.view.clone())));

    b.on_load_apps(|| {
        std::thread::spawn(|| {
            let apps: Vec<AppData> = discover_apps()
                .into_iter()
                .map(|app| AppData {
                    id: app
                        .id()
                        .map(|i| i.to_string())
                        .unwrap_or_default(),
                    name: app.name().to_string(),
                    icon_path: app
                        .icon()
                        .and_then(|i| gio_icon_to_path(&i, 64)),
                })
                .collect();

            let _ = slint::invoke_from_event_loop(move || {
                let items: Vec<OpenWithApp> = apps
                    .into_iter()
                    .map(|a| OpenWithApp {
                        icon: a
                            .icon_path
                            .and_then(|p| {
                                Image::load_from_path(&p).ok()
                            })
                            .unwrap_or_default(),
                        name: a.name.to_shared_string(),
                        id: a.id.to_shared_string(),
                    })
                    .collect();

                STATE.with(|s| *s.items.borrow_mut() = items);
                filter();
            });
        });
    });

    b.on_filter(|text| {
        STATE.with(|s| {
            *s.needle.borrow_mut() = text.trim().to_string();
        });
        filter();
    });

    b.on_clear_filter(|| {
        STATE.with(|s| s.needle.borrow_mut().clear());
        filter();
    });

    b.on_confirm(move |idx| {
        let Ok(idx) = usize::try_from(idx) else {
            return;
        };
        let Some(app) = STATE.with(|s| s.view.row_data(idx)) else {
            return;
        };

        match launch_app(app.id.as_str()) {
            Ok(()) => {
                if let Some(w) = weak.upgrade() {
                    w.global::<OpenWithBridge>().set_open(false);
                }
            }
            Err(e) => eprintln!("Failed to launch {}: {e}", app.name),
        }
    });
}
