use crate::NectanWindow;
use crate::drag_and_drop_handler::{MIME, UiEvent};
use crate::uri::file_uri;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, CURRENT_TIME};

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        XdndAware, XdndEnter, XdndPosition, XdndStatus, XdndLeave, XdndDrop,
        XdndFinished, XdndSelection, XdndActionCopy, TARGETS,
        TEXT_URI_LIST: b"text/uri-list",
    }
}

struct Session {
    target: Option<Window>,
    target_ver: u32,
    accepted: bool,
    last_pos: (i16, i16),
    dropped_at: Option<Instant>,
}

pub struct X11Drag {
    window_id: u32,
    conn: RustConnection,
    root: Window,
    win: Window,
    atoms: Atoms,
    uri_data: Vec<u8>,
    session: Option<Session>,
}

impl X11Drag {
    pub fn new(app: &NectanWindow) -> Result<Self, String> {
        let h = app.window().window_handle();
        let window_id =
            match h.window_handle().map_err(|e| e.to_string())?.as_raw() {
                RawWindowHandle::Xlib(x) => x.window as u32,
                RawWindowHandle::Xcb(x) => x.window.get(),
                _ => panic!("Wtf"),
            };

        let (conn, screen_num) =
            RustConnection::connect(None).map_err(|e| e.to_string())?;
        let root = conn.setup().roots[screen_num].root;
        let win = conn.generate_id().map_err(|e| e.to_string())?;
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            win,
            root,
            0, // sussy ??? TODO CHECK THIS SHIT
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            COPY_FROM_PARENT,
            &CreateWindowAux::new(),
        )
        .map_err(|e| e.to_string())?;
        let atoms = Atoms::new(&conn)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        conn.flush().map_err(|e| e.to_string())?;
        Ok(Self {
            window_id,
            conn,
            root,
            win,
            atoms,
            uri_data: Vec::new(),
            session: None,
        })
    }

    pub fn start_drag(&mut self, paths: &[PathBuf]) {
        if self.session.is_some() {
            return;
        }
        // Only start while the left button is actually held down.
        if !self.button1_down() {
            return;
        }
        self.uri_data = paths
            .iter()
            .map(|p| file_uri(p))
            .collect::<String>()
            .into_bytes();
        let _ = self.conn.set_selection_owner(
            self.win,
            self.atoms.XdndSelection,
            CURRENT_TIME,
        );
        let _ = self.conn.flush();
        self.session = Some(Session {
            target: None,
            target_ver: 0,
            accepted: false,
            last_pos: (-1, -1),
            dropped_at: None,
        });
        eprintln!("x11 drag started");
    }

    fn button1_down(&self) -> bool {
        self.conn
            .query_pointer(self.root)
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some_and(|q| {
                u16::from(q.mask) & u16::from(KeyButMask::BUTTON1) != 0
            })
    }

    /// Pointer position relative to one of our windows, in physical pixels.
    pub fn pointer_in_window(&self) -> Option<(f32, f32)> {
        let w = self.window_id;
        let q = self.conn.query_pointer(w).ok()?.reply().ok()?;
        Some((q.win_x as f32, q.win_y as f32))
    }

    fn send(&self, target: Window, ty: u32, data: [u32; 5]) {
        let ev = ClientMessageEvent::new(32, target, ty, data);
        let _ = self.conn.send_event(false, target, EventMask::NO_EVENT, ev);
    }

    fn aware_version(&self, w: Window) -> Option<u32> {
        let r = self
            .conn
            .get_property(false, w, self.atoms.XdndAware, AtomEnum::ATOM, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        let version = r.value32()?.next();
        version
    }

    /// Walk down from the root under the pointer to the first XdndAware window.
    fn find_target(&self) -> Option<(Window, u32)> {
        let mut win = self.root;
        for _ in 0..16 {
            let q = self.conn.query_pointer(win).ok()?.reply().ok()?;
            if q.child == 0 {
                return None;
            }
            if let Some(v) = self.aware_version(q.child) {
                return Some((q.child, v));
            }
            win = q.child;
        }
        None
    }

    fn handle_events(&mut self) {
        while let Ok(Some(ev)) = self.conn.poll_for_event() {
            match ev {
                Event::SelectionRequest(e)
                    if e.selection == self.atoms.XdndSelection =>
                {
                    let prop = if e.property == 0 {
                        e.target
                    } else {
                        e.property
                    };
                    let mut ok = true;
                    if e.target == self.atoms.TEXT_URI_LIST {
                        let _ = self.conn.change_property8(
                            PropMode::REPLACE,
                            e.requestor,
                            prop,
                            self.atoms.TEXT_URI_LIST,
                            &self.uri_data,
                        );
                    } else if e.target == self.atoms.TARGETS {
                        let _ = self.conn.change_property32(
                            PropMode::REPLACE,
                            e.requestor,
                            prop,
                            AtomEnum::ATOM,
                            &[self.atoms.TEXT_URI_LIST, self.atoms.TARGETS],
                        );
                    } else {
                        ok = false;
                    }
                    let notify = SelectionNotifyEvent {
                        response_type: SELECTION_NOTIFY_EVENT,
                        sequence: 0,
                        time: e.time,
                        requestor: e.requestor,
                        selection: e.selection,
                        target: e.target,
                        property: if ok { prop } else { 0 },
                    };
                    let _ = self.conn.send_event(
                        false,
                        e.requestor,
                        EventMask::NO_EVENT,
                        notify,
                    );
                    eprintln!("x11: served selection request (ok={ok})");
                }
                Event::ClientMessage(m) if m.type_ == self.atoms.XdndStatus => {
                    let d = m.data.as_data32();
                    if let Some(s) = self.session.as_mut() {
                        if s.target == Some(d[0]) {
                            s.accepted = d[1] & 1 != 0;
                        }
                    }
                }
                Event::ClientMessage(m)
                    if m.type_ == self.atoms.XdndFinished =>
                {
                    eprintln!("x11: drop finished");
                    self.session = None;
                }
                _ => {}
            }
        }
        let _ = self.conn.flush();
    }

    pub fn pump(&mut self) {
        self.handle_events();
        let Some(mut s) = self.session.take() else {
            return;
        };

        // After the drop, wait for XdndFinished (or give up).
        if let Some(t) = s.dropped_at {
            if t.elapsed() < Duration::from_secs(3) {
                self.session = Some(s);
            }
            return;
        }

        let Ok(Ok(q)) = self.conn.query_pointer(self.root).map(|c| c.reply())
        else {
            self.session = Some(s);
            return;
        };
        let pressed = u16::from(q.mask) & u16::from(KeyButMask::BUTTON1) != 0;
        let (target, ver) =
            self.find_target().map_or((None, 0), |(w, v)| (Some(w), v));

        if target != s.target {
            if let Some(old) = s.target {
                self.send(old, self.atoms.XdndLeave, [self.win, 0, 0, 0, 0]);
            }
            s.target = target;
            s.target_ver = ver;
            s.accepted = false;
            s.last_pos = (-1, -1);
            if let Some(t) = target {
                self.send(
                    t,
                    self.atoms.XdndEnter,
                    [
                        self.win,
                        ver.min(5) << 24,
                        self.atoms.TEXT_URI_LIST,
                        0,
                        0,
                    ],
                );
            }
        }

        if pressed {
            if let Some(t) = s.target {
                if s.last_pos != (q.root_x, q.root_y) {
                    s.last_pos = (q.root_x, q.root_y);
                    let xy = (((q.root_x as u32) & 0xffff) << 16)
                        | ((q.root_y as u32) & 0xffff);
                    self.send(
                        t,
                        self.atoms.XdndPosition,
                        [
                            self.win,
                            0,
                            xy,
                            CURRENT_TIME,
                            self.atoms.XdndActionCopy,
                        ],
                    );
                }
            }
            self.session = Some(s);
        } else {
            // Button released: drop if the target said it accepts, otherwise leave.
            match s.target {
                Some(t) if s.accepted => {
                    self.send(
                        t,
                        self.atoms.XdndDrop,
                        [self.win, 0, CURRENT_TIME, 0, 0],
                    );
                    s.dropped_at = Some(Instant::now());
                    self.session = Some(s);
                    eprintln!("x11: drop sent");
                }
                Some(t) => {
                    self.send(t, self.atoms.XdndLeave, [self.win, 0, 0, 0, 0]);
                    eprintln!(
                        "x11: released over a target that did not accept"
                    );
                }
                None => eprintln!("x11: released over nothing"),
            }
        }
        let _ = self.conn.flush();
    }
}
