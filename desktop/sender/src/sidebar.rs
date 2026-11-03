use slint::{DataTransfer, Global, SharedString};
use std::rc::Rc;

use crate::{App, SidebarBridge};

pub fn handle_sidebar(w: &App) {
    let b = SidebarBridge::get(w);

    b.on_drag_device(|id: SharedString| {
        let mut t = DataTransfer::default();
        t.set_user_data(Rc::new(id));
        t
    });

    b.on_data_transfer_to_id(|data| {
        data.user_data()
            .and_then(|d| d.downcast::<SharedString>().ok())
            .map(|s| (*s).clone())
            .unwrap_or_default()
    });
}
