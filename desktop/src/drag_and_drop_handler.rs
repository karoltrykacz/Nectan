use crate::state::ui_state;
use crate::{HoverBridge, NectanWindow, wayland, x11};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::TimerMode;
use slint::winit_030::WinitWindowAccessor;
use slint::{
    ComponentHandle,
    winit_030::{EventResult, winit},
};
use std::time::Duration;
use std::{cell::RefCell, path::PathBuf, rc::Rc};
use tracing::{error, info};

pub const MIME: &str = "text/uri-list";

pub enum UiEvent {
    Hover(bool),
    Pos(f64, f64),
    Received(String, f64, f64),
}

enum Dnd {
    // TODO
    Wayland(wayland::Wl),
    X11(x11::X11Drag),
}

impl Dnd {
    fn new(window: &NectanWindow) -> Result<Self, String> {
        let handle = window.window().window_handle();
        match handle.window_handle().map_err(|e| e.to_string())?.as_raw() {
            RawWindowHandle::Wayland(_) => {
                println!("wayland");
                Ok(Dnd::Wayland(wayland::Wl::new(window)?))
            }
            RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_) => {
                println!("x11");
                Ok(Dnd::X11(x11::X11Drag::new(window)?))
            }
            other => Err(format!("unsupported window system: {other:?}")),
        }
    }
    fn pump(&mut self) {
        match self {
            Dnd::Wayland(w) => w.pump(),
            Dnd::X11(x) => x.pump(),
        }
    }
    fn x11_pointer(&self) -> Option<(f32, f32)> {
        match self {
            Dnd::X11(x) => x.pointer_in_window(),
            _ => None,
        }
    }
    fn take_ui(&mut self) -> Vec<UiEvent> {
        match self {
            Dnd::Wayland(w) => w.take_ui(),
            Dnd::X11(_) => Vec::new(),
        }
    }
    fn start_drag(&mut self, path: PathBuf) {
        match self {
            Dnd::Wayland(w) => w.start_drag(path),
            Dnd::X11(x) => x.start_drag(&[path]),
        }
    }
}

fn hook_winit_drops(w: &NectanWindow) {
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
                b.set_hovering(false);
                // No position in winit's event: use the last polled pointer position.
                // app.invoke_accept(
                //     p.display().to_string().into(),
                //     app.get_hover_x(),
                //     app.get_hover_y(),
                // );
            }
            _ => {}
        }
        EventResult::Propagate
    });
}
pub fn handle_drag_drop(w: &NectanWindow) {
    hook_winit_drops(&w);

    let dnd: Rc<RefCell<Option<Dnd>>> = Rc::new(RefCell::new(None));
    {
        let dnd = dnd.clone();

        // callback start-drag();

        // .on_start_drag(move || {
        //     if let Some(d) = dnd.borrow_mut().as_mut() {
        //         d.start_drag(path.clone());
        //     }
        // });
    }
    let weak = w.as_weak();
    let ui = ui_state();
    {
        let dnd = dnd.clone();
        ui.dnd_init_timer.borrow().start(
            TimerMode::SingleShot,
            Duration::from_millis(300),
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

    let dnd = dnd.clone();
    let weak = w.as_weak();
    ui.dnd_pump_timer.borrow().start(
        TimerMode::Repeated,
        Duration::from_millis(41),
        move || {
            let (events, x11pos) = match dnd.borrow_mut().as_mut() {
                Some(d) => {
                    d.pump();
                    (d.take_ui(), d.x11_pointer())
                }
                None => return,
            };

            let Some(w) = weak.upgrade() else { return };
            let b = w.global::<HoverBridge>();

            // X11: winit gives no drag position, so read it from the X server while hovering.
            if let (Some((px, py)), true) = (x11pos, b.get_hovering()) {
                println!("Hovering (x11) {} {}", px, py);
                let sf = w.window().scale_factor();
                b.set_hover_x(px / sf);
                b.set_hover_y(py / sf);
                println!("Hovering (x11) {} {}", px / sf, py / sf);
            }

            for ev in events {
                match ev {
                    UiEvent::Hover(h) => b.set_hovering(h),
                    UiEvent::Pos(x, y) => {
                        b.set_hover_x(x as f32);
                        println!("Hovering (way) {x} {y}");
                        b.set_hover_y(y as f32);
                    }
                    UiEvent::Received(text, x, y) => {
                        eprintln!("dropped at ({x:.0}, {y:.0}): {text}");
                        // TODO!
                        // app.invoke_accept(text.into(), x as f32, y as f32);
                    }
                }
            }
        },
    );
}
