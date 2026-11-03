use crate::state::ui_state;
use crate::transfers::open_send_modal;
use crate::{App, HoverBridge, wayland};
use nectan_core::devices::device_id_from_base64;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::winit_030::WinitWindowAccessor;
use slint::{
    ComponentHandle,
    winit_030::{EventResult, winit},
};
use slint::{Global, LogicalPosition, TimerMode};
use std::time::Duration;
use std::{cell::RefCell, path::PathBuf, rc::Rc};
use tracing::{error, info};
use uuid::Uuid;

pub const MIME: &str = "text/uri-list";

pub enum UiEvent {
    Hover(bool),
    Pos(f64, f64),
    Received(Vec<PathBuf>, f64, f64),
}

enum Dnd {
    // TODO - windows
    Wayland(wayland::Wl),
}

impl Dnd {
    fn new(window: &App) -> Result<Self, String> {
        let handle = window.window().window_handle();
        match handle
            .window_handle()
            .map_err(|e| e.to_string())?
            .as_raw()
        {
            RawWindowHandle::Wayland(_) => {
                println!("wayland");
                Ok(Dnd::Wayland(wayland::Wl::new(window)?))
            }

            other => {
                Err(format!("unsupported window system: {other:?}"))
            }
        }
    }
    fn pump(&mut self) {
        match self {
            Dnd::Wayland(w) => w.pump(),
        }
    }
    fn take_ui(&mut self) -> Vec<UiEvent> {
        match self {
            Dnd::Wayland(w) => w.take_ui(),
        }
    }
    fn start_drag(&mut self, path: PathBuf) {
        match self {
            Dnd::Wayland(w) => w.start_drag(path),
        }
    }
}

fn hook_winit_drops(w: &App) {
    let weak = w.as_weak();
    w.window().on_winit_window_event(move |_w, event| {
        use winit::event::WindowEvent as WE;
        let Some(w) = weak.upgrade() else {
            return EventResult::Propagate;
        };
        let b = w.global::<HoverBridge>();

        match event {
            WE::HoveredFile(_) => b.set_hovering(true),
            WE::HoveredFileCancelled => b.set_hovering(false),
            WE::DroppedFile(p) => {
                println!("Dropped file {p:?}");
                // b.set_hovering(false);
            }
            _ => {}
        }
        EventResult::Propagate
    });
}
pub fn handle_drag_drop(w: &App) {
    hook_winit_drops(&w);

    let weak = w.as_weak();
    let paths: Rc<RefCell<Vec<PathBuf>>> =
        Rc::new(RefCell::new(Vec::new()));

    let p = paths.clone();
    HoverBridge::get(&w).on_dropped(move |id| {
        println!("Dropped on {id}");
        if let Ok(id) = device_id_from_base64(&id) {
            println!("DeviceId valid");
        }
    });

    let dnd: Rc<RefCell<Option<Dnd>>> = Rc::new(RefCell::new(None));
    let weak = w.as_weak();
    let ui = ui_state();
    let dnd_init_timer = slint::Timer::default();
    {
        let dnd = dnd.clone();
        dnd_init_timer.start(
            TimerMode::SingleShot,
            Duration::from_millis(1000),
            move || {
                let w = weak.upgrade().unwrap();
                match Dnd::new(&w) {
                    Ok(d) => {
                        println!("dnd ready");
                        *dnd.borrow_mut() = Some(d);
                    }
                    Err(e) => eprintln!("dnd init failed: {e}"),
                }
            },
        );
    }
    std::mem::forget(dnd_init_timer);

    let dnd = dnd.clone();
    let weak = w.as_weak();
    ui.dnd_timer.borrow().start(
        TimerMode::Repeated,
        Duration::from_millis(41),
        move || {
            let events = match dnd.borrow_mut().as_mut() {
                Some(d) => {
                    d.pump();
                    d.take_ui()
                }
                None => return,
            };

            let Some(w) = weak.upgrade() else { return };
            let b = w.global::<HoverBridge>();

            for ev in events {
                match ev {
                    UiEvent::Hover(h) => b.set_hovering(h),
                    UiEvent::Pos(x, y) => {
                        b.set_hover_x(x as f32);
                        b.set_hover_y(y as f32);
                    }
                    UiEvent::Received(contents, x, y) => {
                        open_send_modal(weak.clone(), contents);
                    }
                }
            }
        },
    );
}
