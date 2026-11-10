use crate::common_storage::StoredShortcut;
use slint::Keys;

const DEFAULT_SHORTCUTS: &[(&str, &[&str], &[&str])] = &[
    ("close", &["Escape"], &[]),
    ("confirm", &["Return"], &[]),
    ("move-down", &["J"], &["DownArrow"]),
    ("move-up", &["K"], &["UpArrow"]),
    ("go-parent", &["H"], &["LeftArrow"]),
    ("go-inside", &["L"], &["Return"]),
    ("essentials", &["E"], &[]),
    ("toggle-sidebar", &["Control", "B"], &[]),
    ("transfers", &["Control", "T"], &[]),
    ("settings", &["Control", "I"], &[]),
    ("go-up", &["H"], &["Alt", "UpArrow"]),
    ("go-down", &["L"], &["Control", "DownArrow"]),
    ("history-forward", &["Control", "I"], &["Alt", "RightArrow"]),
    ("history-back", &["Control", "O"], &["Control", "LeftArrow"]),
    ("half-page-down", &["Control", "D"], &["PageDown"]),
    ("half-page-up", &["Control", "U"], &["PageUp"]),
    ("search-in-current", &["Slash"], &["Control", "F"]),
    ("search-recursively", &["Shift", "Slash"], &["Shift", "F"]),
    (
        "ripgrep",
        &["Control", "Shift", "Slash"],
        &["Control", "Shift", "F"],
    ),
    ("copy", &["Y"], &["Control", "C"]),
    ("next", &["N"], &[]),
    ("previous", &["Shift", "N"], &[]),
    ("cut", &["D"], &["Control", "X"]),
    ("trash", &["Shift", "D"], &["Delete"]),
    ("delete-permanently", &["Control", "Shift", "D"], &[]),
    ("select-all", &["Control", "A"], &[]),
    ("select-regex", &["Control", "S"], &[]),
    ("rename", &["R"], &["F2"]),
    ("properties", &["Alt", "Return"], &[]),
    ("refresh", &["Control", "L"], &["F5"]),
    ("create", &["Control", "Shift", "N"], &[]),
    ("toggle-hidden", &["Control", "H"], &[]),
    ("list-view", &["Control", "Digit1"], &[]),
    ("grid-view", &["Control", "Digit2"], &[]),
    ("zoom-out", &["Control", "HyphenMinus"], &[]),
    ("zoom-in", &["Control", "Plus"], &[]),
];

pub fn default_shortcut(id: &str) -> StoredShortcut {
    DEFAULT_SHORTCUTS
        .iter()
        .find(|(i, ..)| *i == id)
        .map(|(_, k, a)| StoredShortcut {
            keys: k.iter().map(|s| s.to_string()).collect(),
            alt_keys: a.iter().map(|s| s.to_string()).collect(),
        })
        .unwrap_or_default()
}

pub fn parts_to_keys(parts: &[String]) -> Keys {
    if parts.is_empty() {
        return Keys::default();
    }
    Keys::from_parts(parts.iter().map(String::as_str))
        .unwrap_or_default()
}
