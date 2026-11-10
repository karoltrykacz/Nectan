use super::CommonStorage;
use crate::App;
use crate::Arc;
use crate::DropDownOption;
use crate::WindowBridge;
use slint::{
    ComponentHandle, Global, ModelRc, ToSharedString, VecModel,
};

pub const LANGUAGES: [(&str, &str); 2] =
    [("English", "en"), ("Polski", "pl")];

pub fn init_lang(w: &App, storage: Arc<CommonStorage>) {
    let b = w.global::<WindowBridge>();
    let saved = storage.lang();
    let idx = LANGUAGES
        .iter()
        .position(|(_, code)| *code == saved)
        .unwrap_or(0);
    b.set_selected_language_idx(idx as i32);
    apply_language(&saved);

    b.set_languages(ModelRc::new(VecModel::from(language_options())));

    let w = WindowBridge::get(w);

    let idx = LANGUAGES
        .iter()
        .position(|(_, code)| *code == saved)
        .unwrap_or(0);

    w.set_selected_language_idx(idx as i32);
    apply_language(&saved);

    b.on_select_language({
        move |code| {
            apply_language(code.as_str());
            if let Err(e) =
                storage.update(|s| s.language = code.to_string())
            {
                eprintln!("Failed to save language: {e}");
            }
        }
    });
}

fn language_options() -> Vec<DropDownOption> {
    LANGUAGES
        .iter()
        .map(|(name, code)| DropDownOption {
            name: (*name).into(),
            code: (*code).into(),
        })
        .collect()
}

fn apply_language(code: &str) {
    let lang = if code == "en" { "" } else { code };
    if let Err(e) = slint::select_bundled_translation(lang) {
        println!("Failed to select language [{code}] {e}");
    }
}
