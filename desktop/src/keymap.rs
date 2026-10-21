use std::sync::Arc;

use nectan_core::storage::{Storage, default_shortcut};
use slint::{ComponentHandle, Keys as SlintKeys};

use crate::{KeyMap, Keys, NectanWindow, Shortcut};

pub fn handle_keys(w: &NectanWindow, storage: Arc<Storage>) {
    let keys = w.global::<Keys>();

    keys.set_map(build_keymap(&storage));

    let weak = w.as_weak();

    let s = storage.clone();
    keys.on_save_shortcut(move |id, k, is_primary| {
        let storage = s.clone();
        let parts: Vec<String> = k.to_parts().map(|p| p.to_string()).collect();
        if let Err(e) = storage.set_shortcut(&id, is_primary, parts) {
            return;
        }
        if let Some(w) = weak.upgrade() {
            w.global::<Keys>().set_map(build_keymap(&storage));
        }
    });

    fn is_modifier_key(c: char) -> bool {
        matches!(c as u32, 0x10..=0x18)
    }

    pub fn parts_to_keys(parts: &[String]) -> SlintKeys {
        SlintKeys::from_parts(parts.iter().map(String::as_str))
            .unwrap_or_default()
    }

    fn special_key_name(c: char) -> Option<&'static str> {
        Some(match c as u32 {
            0x08 => "Backspace",
            0x09 => "Tab",
            0x0a => "Return",
            0x1b => "Escape",
            0x20 => "Space",
            0x7f => "Delete",
            0xF700 => "UpArrow",
            0xF701 => "DownArrow",
            0xF702 => "LeftArrow",
            0xF703 => "RightArrow",
            0xF704..=0xF71B => {
                return Some(F_KEYS[(c as u32 - 0xF704) as usize]);
            }
            0xF727 => "Insert",
            0xF729 => "Home",
            0xF72B => "End",
            0xF72C => "PageUp",
            0xF72D => "PageDown",
            _ => return None,
        })
    }

    const F_KEYS: [&str; 24] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11",
        "F12", "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21",
        "F22", "F23", "F24",
    ];

    fn record_keys(
        text: &str,
        ctrl: bool,
        alt: bool,
        shift: bool,
        meta: bool,
    ) -> SlintKeys {
        let Some(ch) = text.chars().next() else {
            return SlintKeys::default();
        };
        if is_modifier_key(ch) {
            return SlintKeys::default();
        }

        let key: String = if let Some(name) = special_key_name(ch) {
            name.to_string()
        } else if (ch as u32) < 0x20 {
            (((ch as u8) + 0x60) as char)
                .to_ascii_uppercase()
                .to_string()
        } else {
            ch.to_uppercase().collect()
        };

        let mut parts: Vec<&str> = Vec::with_capacity(5);
        if ctrl {
            parts.push("Control");
        }
        if alt {
            parts.push("Alt");
        }
        if shift {
            parts.push("Shift");
        }
        if meta {
            parts.push("Meta");
        }
        parts.push(&key);

        match SlintKeys::from_parts(parts) {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!("couldn't build keys from {text:?}: {e:?}");
                SlintKeys::default()
            }
        }
    }

    keys.on_make_keys(move |event| {
        record_keys(
            &event.text,
            event.modifiers.control,
            event.modifiers.alt,
            event.modifiers.shift,
            event.modifiers.meta,
        )
    });

    let weak = w.as_weak();
    keys.on_save_shortcut(move |id, keys, is_primary| {
        let parts: Vec<String> =
            keys.to_parts().map(|p| p.to_string()).collect();
        println!("Saved {parts:?}");
        if let Err(e) = storage.set_shortcut(&id, is_primary, parts) {
            tracing::error!("failed to save shortcut {id}: {e:?}");
        }
        let w = weak.upgrade().unwrap();
        let keys = w.global::<Keys>();
        keys.set_map(build_keymap(&storage));
    });
}

fn parts_to_keys(parts: &[String]) -> SlintKeys {
    if parts.is_empty() {
        return SlintKeys::default();
    }
    SlintKeys::from_parts(parts.iter().map(String::as_str)).unwrap_or_default()
}

fn build_keymap(storage: &Storage) -> KeyMap {
    let saved = storage.shortcuts();
    let sc = |id: &str| {
        let s = saved
            .get(id)
            .cloned()
            .unwrap_or_else(|| default_shortcut(id));
        Shortcut {
            keys: parts_to_keys(&s.keys),
            alt_keys: parts_to_keys(&s.alt_keys),
        }
    };

    KeyMap {
        close: sc("close"),
        confirm: sc("confirm"),
        move_down: sc("move-down"),
        move_up: sc("move-up"),
        essentials: sc("essentials"),
        toggle_sidebar: sc("toggle-sidebar"),
        transfers: sc("transfers"),
        settings: sc("settings"),
        go_up: sc("go-up"),
        go_down: sc("go-down"),
        history_forward: sc("history-forward"),
        history_back: sc("history-back"),
        half_page_down: sc("half-page-down"),
        half_page_up: sc("half-page-up"),
        find: sc("find"),
        ripgrep: sc("ripgrep"),
        copy: sc("copy"),
        cut: sc("cut"),
        trash: sc("trash"),
        delete_permanently: sc("delete-permanently"),
        select_all: sc("select-all"),
        select_regex: sc("select-regex"),
        rename: sc("rename"),
        properties: sc("properties"),
        refresh: sc("refresh"),
        create: sc("create"),
        toggle_hidden: sc("toggle-hidden"),
        list_view: sc("list-view"),
        grid_view: sc("grid-view"),
        zoom_out: sc("zoom-out"),
        zoom_in: sc("zoom-in"),
    }
}
