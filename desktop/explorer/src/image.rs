use slint::ComponentHandle;
use slint::Image;
use std::cell::RefCell;

use crate::{ImageApp, theme::init_theme};

thread_local! {
    static IMAGE_WINDOW: RefCell<Option<ImageApp>> = RefCell::new(None);
}

pub fn image_viewer() -> Result<(), slint::PlatformError> {
    let image_w = ImageApp::new()?;
    init_theme(&image_w);
    image_w.show()?;
    let path = std::path::PathBuf::from(
        "/home/karol/Downloads/nature-beauty-colors-meadow-daisy-blossoms-generated-by-ai.jpg",
    );
    let image = Image::load_from_path(&path).unwrap();
    image_w.set_image(image);

    IMAGE_WINDOW.with(|w| *w.borrow_mut() = Some(image_w));
    Ok(())
}
