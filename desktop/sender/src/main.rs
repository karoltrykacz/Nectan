#[cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let w = NectanSender::new()?;
    w.on_new_window(|| {
        let w = NectanSender::new().unwrap();
        w.show();
    });
    w.run()
}
