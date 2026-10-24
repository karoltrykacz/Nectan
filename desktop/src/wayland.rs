use crate::NectanWindow;
use crate::drag_and_drop_handler::{MIME, UiEvent};
use raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};

use slint::ComponentHandle;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Mutex;
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::{
    DndAction, WlDataDeviceManager,
};
use wayland_client::protocol::wl_data_offer::{self, WlDataOffer};
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
use wayland_client::protocol::wl_pointer::{self, ButtonState, WlPointer};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    event_created_child,
};

use crate::uri::{decode_uri_list, file_uri};

type OfferData = Mutex<Vec<String>>;

struct PendingRead {
    sock: UnixStream,
    data: Vec<u8>,
    pos: (f64, f64),
    offer: WlDataOffer,
}

struct State {
    window: WlSurface,
    press_serial: Option<u32>,
    pointer_surface: Option<WlSurface>,
    drag_path: PathBuf,
    source: Option<WlDataSource>,
    // incoming drag
    offer: Option<WlDataOffer>,
    reads: Vec<PendingRead>,
    last_pos: (f64, f64),
    ui: Vec<UiEvent>,
}

pub struct Wl {
    conn: Connection,
    queue: EventQueue<State>,
    state: State,
    manager: WlDataDeviceManager,
    device: WlDataDevice,
    _seat: WlSeat,
    _pointer: WlPointer,
}

impl Wl {
    pub fn new(window: &NectanWindow) -> Result<Self, String> {
        let slint_handle = window.window().window_handle();

        let display_ptr = match HasDisplayHandle::display_handle(&slint_handle)
            .map_err(|e| e.to_string())?
            .as_raw()
        {
            RawDisplayHandle::Wayland(h) => h.display.as_ptr(),
            _ => return Err("not running on Wayland (X11 session?)".into()),
        };
        let backend =
            unsafe { Backend::from_foreign_display(display_ptr.cast()) };
        let conn = Connection::from_backend(backend);

        let surface_ptr = match HasWindowHandle::window_handle(&slint_handle)
            .map_err(|e| e.to_string())?
            .as_raw()
        {
            RawWindowHandle::Wayland(h) => h.surface.as_ptr(),
            _ => return Err("window is not a Wayland window".into()),
        };
        let id = unsafe {
            ObjectId::from_ptr(WlSurface::interface(), surface_ptr.cast())
        }
        .map_err(|e| e.to_string())?;

        let window =
            WlSurface::from_id(&conn, id).map_err(|e| e.to_string())?;

        let (globals, mut queue) =
            registry_queue_init::<State>(&conn).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        let seat: WlSeat =
            globals.bind(&qh, 1..=7, ()).map_err(|e| e.to_string())?;
        let manager: WlDataDeviceManager =
            globals.bind(&qh, 1..=3, ()).map_err(|e| e.to_string())?;
        let pointer = seat.get_pointer(&qh, ());
        let device = manager.get_data_device(&seat, &qh, ());

        let mut state = State {
            window,
            press_serial: None,
            pointer_surface: None,
            drag_path: PathBuf::new(),
            source: None,
            offer: None,
            reads: Vec::new(),
            last_pos: (0.0, 0.0),
            ui: Vec::new(),
        };
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        Ok(Wl {
            conn,
            queue,
            state,
            manager,
            device,
            _seat: seat,
            _pointer: pointer,
        })
    }

    pub fn pump(&mut self) {
        let _ = self.queue.dispatch_pending(&mut self.state);
        // Progress incoming reads (non-blocking, so our own drags can answer themselves).
        let mut i = 0;
        while i < self.state.reads.len() {
            let mut done = false;
            let r = &mut self.state.reads[i];
            let mut buf = [0u8; 4096];
            loop {
                match r.sock.read(&mut buf) {
                    Ok(0) => {
                        done = true;
                        break;
                    }
                    Ok(n) => r.data.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        break;
                    }
                    Err(_) => {
                        done = true;
                        break;
                    }
                }
            }
            if done {
                let r = self.state.reads.remove(i);
                let text = String::from_utf8_lossy(&r.data).to_string();
                let paths = decode_uri_list(&text);
                self.state.ui.push(UiEvent::Received(
                    paths.join(", "),
                    r.pos.0,
                    r.pos.1,
                ));
                r.offer.finish();
                r.offer.destroy();
            } else {
                i += 1;
            }
        }
        let _ = self.conn.flush();
    }

    pub fn take_ui(&mut self) -> Vec<UiEvent> {
        std::mem::take(&mut self.state.ui)
    }

    pub fn start_drag(&mut self, path: PathBuf) {
        self.pump();
        let Some(serial) = self.state.press_serial.take() else {
            return;
        };
        let Some(origin) = self.state.pointer_surface.clone() else {
            return;
        };
        self.state.drag_path = path;
        let qh = self.queue.handle();
        let source = self.manager.create_data_source(&qh, ());

        source.offer(MIME.into());
        source.set_actions(DndAction::Copy);

        self.device.start_drag(Some(&source), &origin, None, serial);
        self.state.source = Some(source);
        let _ = self.conn.flush();
        eprintln!("start_drag issued (serial {serial})");
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wayland_client::protocol::wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<WlPointer, ()> for State {
    fn event(
        state: &mut Self,
        _: &WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter { surface, .. } => {
                state.pointer_surface = Some(surface)
            }
            wl_pointer::Event::Button {
                serial,
                button,
                state: WEnum::Value(ButtonState::Pressed),
                ..
            } if button == 0x110 => {
                state.press_serial = Some(serial);
            }
            _ => {}
        }
    }
}
impl Dispatch<WlDataDeviceManager, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlDataDeviceManager,
        _: wayland_client::protocol::wl_data_device_manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<WlDataDevice, ()> for State {
    fn event(
        state: &mut Self,
        _: &WlDataDevice,
        event: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_device::Event::Enter {
                serial,
                surface,
                x,
                y,
                id,
            } => {
                let wants = id.as_ref().is_some_and(|o| {
                    o.data::<OfferData>().is_some_and(|d| {
                        d.lock().unwrap().iter().any(|m| m == MIME)
                    })
                });
                if let (Some(offer), true) = (id, wants) {
                    offer.accept(serial, Some(MIME.to_string()));
                    if offer.version() >= 3 {
                        offer.set_actions(DndAction::Copy, DndAction::Copy);
                    }
                    state.offer = Some(offer);
                    state.last_pos = (x, y);
                    state.ui.push(UiEvent::Pos(x, y));
                    state.ui.push(UiEvent::Hover(true));
                } else {
                    state.offer = None;
                }
            }
            wl_data_device::Event::Motion { x, y, .. } => {
                state.last_pos = (x, y);
                state.ui.push(UiEvent::Pos(x, y));
            }
            wl_data_device::Event::Leave => {
                state.ui.push(UiEvent::Hover(false));
                if let Some(o) = state.offer.take() {
                    o.destroy();
                }
            }
            wl_data_device::Event::Drop => {
                if let Some(offer) = state.offer.take() {
                    state.ui.push(UiEvent::Hover(false));
                    if let Ok((ours, theirs)) = UnixStream::pair() {
                        offer.receive(MIME.to_string(), theirs.as_fd());
                        drop(theirs);
                        let _ = ours.set_nonblocking(true);
                        let pos = state.last_pos;
                        state.reads.push(PendingRead {
                            sock: ours,
                            data: Vec::new(),
                            pos,
                            offer,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    event_created_child!(State, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, OfferData::default()),
    ]);
}
impl Dispatch<WlDataOffer, OfferData> for State {
    fn event(
        _: &mut Self,
        _: &WlDataOffer,
        event: wl_data_offer::Event,
        data: &OfferData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_offer::Event::Offer { mime_type } = event {
            data.lock().unwrap().push(mime_type);
        }
    }
}
impl Dispatch<WlDataSource, ()> for State {
    fn event(
        state: &mut Self,
        source: &WlDataSource,
        event: wl_data_source::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_source::Event::Send { mime_type, fd } => {
                eprintln!("data requested as {mime_type}");
                if mime_type == MIME {
                    let mut f = std::fs::File::from(fd);
                    let _ = f.write_all(file_uri(&state.drag_path).as_bytes());
                }
            }
            wl_data_source::Event::Cancelled => {
                eprintln!("drag cancelled / rejected");
                source.destroy();
                state.source = None;
            }
            wl_data_source::Event::DndFinished => {
                eprintln!("drop finished");
                source.destroy();
                state.source = None;
            }
            _ => {}
        }
    }
}
