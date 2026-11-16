use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
};

use image::{DynamicImage, RgbaImage, imageops};
use slint::{
    ComponentHandle, Global, Image, PhysicalPosition, Rgba8Pixel,
    SharedPixelBuffer, Weak,
};

use crate::properties::{handle_properties, open_properties};
use crate::{ImageViewer, PropertiesBridge, theme::init_theme};

thread_local! {
    static IMAGE_WINDOW: RefCell<Option<ImageViewer>> = RefCell::new(None);
    static STATE: RefCell<State> = RefCell::new(State::default());
}

static LOAD_GENERATION: AtomicU64 = AtomicU64::new(0);

const EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "ico",
    "qoi", "tga", "dds", "exr", "hdr", "pbm", "pgm", "ppm", "pnm",
    "pam", "ff", "svg", "svgz",
];

#[derive(Default)]
struct State {
    files: Vec<PathBuf>,
    index: usize,
    base: Option<Arc<RgbaImage>>,
    flip_h: bool,
    flip_v: bool,
    edit_tx: Option<mpsc::Sender<Job>>,
}

#[derive(Clone, Copy)]
struct EditParams {
    brightness: f32,
    contrast: f32,
    hue: f32,
    blur: f32,
    sharpen: f32,
    invert: bool,
    flip_h: bool,
    flip_v: bool,
}

struct Job {
    base: Arc<RgbaImage>,
    params: EditParams,
    generation: u64,
}

fn is_vector(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(e.to_ascii_lowercase().as_str(), "svg" | "svgz")
    })
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn set_file_info(w: &ImageViewer, path: &Path) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let size = std::fs::metadata(path)
        .map(|m| format_size(m.len()))
        .unwrap_or_default();
    w.set_file_name(name.into());
    w.set_file_size(size.into());
}

fn shift_brightness(img: &mut RgbaImage, amount: i32) {
    for pixel in img.pixels_mut() {
        for channel in 0..3 {
            pixel.0[channel] = (pixel.0[channel] as i32 + amount)
                .clamp(0, 255) as u8;
        }
    }
}

fn geometry(
    img: &RgbaImage,
    flip_h: bool,
    flip_v: bool,
    turns: i32,
) -> RgbaImage {
    let mut out = if flip_h {
        imageops::flip_horizontal(img)
    } else {
        img.clone()
    };
    if flip_v {
        imageops::flip_vertical_in_place(&mut out);
    }
    match turns.rem_euclid(4) {
        1 => imageops::rotate90(&out),
        2 => imageops::rotate180(&out),
        3 => imageops::rotate270(&out),
        _ => out,
    }
}

fn render(base: &RgbaImage, p: &EditParams, turns: i32) -> RgbaImage {
    let mut img = base.clone();
    if p.brightness != 0.0 {
        shift_brightness(
            &mut img,
            (p.brightness * 2.55).round() as i32,
        );
    }
    if p.contrast != 0.0 {
        img = imageops::contrast(&img, p.contrast);
    }
    if p.hue != 0.0 {
        img = imageops::huerotate(&img, p.hue as i32);
    }
    if p.blur > 0.01 {
        img = imageops::blur(&img, p.blur);
    }
    if p.sharpen > 0.01 {
        img = imageops::unsharpen(&img, p.sharpen * 0.3, 0);
    }
    if p.invert {
        imageops::invert(&mut img);
    }
    geometry(&img, p.flip_h, p.flip_v, turns)
}

fn save_image(
    base: &RgbaImage,
    p: &EditParams,
    turns: i32,
    path: &Path,
) -> image::ImageResult<()> {
    let img = render(base, p, turns);
    let path = if path.extension().is_none() {
        path.with_extension("png")
    } else {
        path.to_path_buf()
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("jpg" | "jpeg") => {
            DynamicImage::ImageRgba8(img).to_rgb8().save(&path)
        }
        _ => img.save(&path),
    }
}

fn to_slint(img: &RgbaImage) -> SharedPixelBuffer<Rgba8Pixel> {
    SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        img.as_raw(),
        img.width(),
        img.height(),
    )
}

fn spawn_edit_worker(weak: Weak<ImageViewer>) -> mpsc::Sender<Job> {
    let (tx, rx) = mpsc::channel::<Job>();
    thread::spawn(move || {
        while let Ok(mut job) = rx.recv() {
            while let Ok(next) = rx.try_recv() {
                job = next;
            }
            let out = render(&job.base, &job.params, 0);
            let buf = to_slint(&out);
            let generation = job.generation;
            let res = weak.upgrade_in_event_loop(move |w| {
                if LOAD_GENERATION.load(Ordering::SeqCst)
                    == generation
                {
                    w.set_image(Image::from_rgba8(buf));
                }
            });
            if res.is_err() {
                break;
            }
        }
    });
    tx
}

fn read_params(
    w: &ImageViewer,
    flip_h: bool,
    flip_v: bool,
) -> EditParams {
    EditParams {
        brightness: w.get_brightness(),
        contrast: w.get_contrast(),
        hue: w.get_hue(),
        blur: w.get_blur(),
        sharpen: w.get_sharpen(),
        invert: w.get_invert(),
        flip_h,
        flip_v,
    }
}

fn run_edits(w: &ImageViewer) {
    STATE.with_borrow(|s| {
        if let (Some(base), Some(tx)) = (&s.base, &s.edit_tx) {
            let _ = tx.send(Job {
                base: base.clone(),
                params: read_params(w, s.flip_h, s.flip_v),
                generation: LOAD_GENERATION.load(Ordering::SeqCst),
            });
        }
    });
}

fn load_current(weak: &Weak<ImageViewer>) {
    let generation =
        LOAD_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let Some(path) =
        STATE.with_borrow(|s| s.files.get(s.index).cloned())
    else {
        return;
    };
    if let Some(w) = weak.upgrade() {
        w.set_loading(true);
        set_file_info(&w, &path);
    }

    if is_vector(&path) {
        let _ = weak.upgrade_in_event_loop(move |w| {
            if LOAD_GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            match Image::load_from_path(&path) {
                Ok(img) => {
                    STATE.with_borrow_mut(|s| s.base = None);
                    w.set_can_edit(false);
                    w.set_image(img);
                }
                Err(e) => eprintln!(
                    "failed to load {}: {e:?}",
                    path.display()
                ),
            }
            w.set_loading(false);
        });
        return;
    }

    let weak = weak.clone();
    thread::spawn(move || {
        let res = image::open(&path).map(|d| d.to_rgba8());
        if LOAD_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        let res = res.map(|img| (to_slint(&img), img));
        let _ = weak.upgrade_in_event_loop(move |w| {
            if LOAD_GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            match res {
                Ok((buf, img)) => {
                    STATE.with_borrow_mut(|s| {
                        s.base = Some(Arc::new(img))
                    });
                    w.set_can_edit(true);
                    w.set_image(Image::from_rgba8(buf));
                }
                Err(e) => eprintln!(
                    "failed to load {}: {e}",
                    path.display()
                ),
            }
            w.set_loading(false);
        });
    });
}

fn list_siblings(path: &Path) -> (Vec<PathBuf>, usize) {
    let mut files: Vec<PathBuf> = path
        .parent()
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| {
                            EXTENSIONS.contains(
                                &e.to_ascii_lowercase().as_str(),
                            )
                        })
                })
                .collect()
        })
        .unwrap_or_default();

    files.sort_by_key(|p| {
        p.file_name().map(|n| n.to_string_lossy().to_lowercase())
    });

    match files.iter().position(|p| p == path) {
        Some(i) => (files, i),
        None => {
            files.insert(0, path.to_path_buf());
            (files, 0)
        }
    }
}

fn sync_nav(w: &ImageViewer) {
    let (prev, next) = STATE
        .with_borrow(|s| (s.index > 0, s.index + 1 < s.files.len()));
    w.set_has_prev(prev);
    w.set_has_next(next);
}

fn reset_edit_values(w: &ImageViewer) {
    w.set_brightness(0.0);
    w.set_contrast(0.0);
    w.set_hue(0.0);
    w.set_blur(0.0);
    w.set_sharpen(0.0);
    w.set_grayscale(false);
    w.set_invert(false);
}

fn navigate(weak: &Weak<ImageViewer>, delta: isize) {
    let moved = STATE.with_borrow_mut(|s| {
        let n = s.index as isize + delta;
        if n < 0 || n as usize >= s.files.len() {
            return false;
        }
        s.index = n as usize;
        s.base = None;
        s.flip_h = false;
        s.flip_v = false;
        true
    });
    if !moved {
        return;
    }
    let Some(w) = weak.upgrade() else { return };
    reset_edit_values(&w);
    sync_nav(&w);
    load_current(weak);

    if PropertiesBridge::get(&w).get_open() {
        if let Some(p) =
            STATE.with_borrow(|s| s.files.get(s.index).cloned())
        {
            open_properties::<ImageViewer>(weak, p);
        }
    }
}

fn wire(w: &ImageViewer) {
    let weak = w.as_weak();
    w.on_previous_requested(move || navigate(&weak, -1));
    let weak = w.as_weak();
    w.on_next_requested(move || navigate(&weak, 1));

    let weak = w.as_weak();
    w.on_info_requested(move || {
        if let Some(p) =
            STATE.with_borrow(|s| s.files.get(s.index).cloned())
        {
            open_properties::<ImageViewer>(&weak, p);
        }
    });

    w.on_print_requested(|| {});

    let weak = w.as_weak();
    w.on_edits_changed(move || {
        if let Some(w) = weak.upgrade() {
            run_edits(&w);
        }
    });

    let weak = w.as_weak();
    w.on_edit_cancelled(move || {
        STATE.with_borrow_mut(|s| {
            s.flip_h = false;
            s.flip_v = false;
        });
        if let Some(w) = weak.upgrade() {
            run_edits(&w);
        }
    });

    let weak = w.as_weak();
    w.on_flip_horizontal(move || {
        STATE.with_borrow_mut(|s| s.flip_h = !s.flip_h);
        if let Some(w) = weak.upgrade() {
            run_edits(&w);
        }
    });

    let weak = w.as_weak();
    w.on_flip_vertical(move || {
        STATE.with_borrow_mut(|s| s.flip_v = !s.flip_v);
        if let Some(w) = weak.upgrade() {
            run_edits(&w);
        }
    });

    let weak = w.as_weak();
    w.on_crop_apply(move || {
        let Some(w) = weak.upgrade() else { return };
        let (cx, cy, cw, ch) = (
            w.get_crop_x(),
            w.get_crop_y(),
            w.get_crop_w(),
            w.get_crop_h(),
        );
        let turns = w.get_turns();

        let cropped = STATE.with_borrow(|s| {
            s.base.as_ref().map(|b| {
                let g = geometry(b, s.flip_h, s.flip_v, turns);
                let (iw, ih) = (g.width(), g.height());
                let x = ((cx * iw as f32).round() as u32).min(iw - 1);
                let y = ((cy * ih as f32).round() as u32).min(ih - 1);
                let width = ((cw * iw as f32).round() as u32)
                    .clamp(1, iw - x);
                let height = ((ch * ih as f32).round() as u32)
                    .clamp(1, ih - y);
                imageops::crop_imm(&g, x, y, width, height).to_image()
            })
        });
        let Some(cropped) = cropped else { return };

        STATE.with_borrow_mut(|s| {
            s.base = Some(Arc::new(cropped));
            s.flip_h = false;
            s.flip_v = false;
        });
        w.set_turns(0);
        w.set_cropping(false);
        run_edits(&w);
    });

    let weak = w.as_weak();
    w.on_save_requested(move || {
        let Some(w) = weak.upgrade() else { return };
        let turns = w.get_turns();
        let Some((base, params, src)) = STATE.with_borrow(|s| {
            s.base.clone().map(|b| {
                (
                    b,
                    read_params(&w, s.flip_h, s.flip_v),
                    s.files.get(s.index).cloned(),
                )
            })
        }) else {
            return;
        };

        let stem = src
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".into());
        let dir = src
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf);

        let weak = weak.clone();
        let _ = slint::spawn_local(async move {
            let mut dlg = rfd::AsyncFileDialog::new()
                .set_title("Save image as")
                .set_file_name(format!("{stem}_edited.png"))
                .add_filter("PNG", &["png"])
                .add_filter("JPEG", &["jpg", "jpeg"])
                .add_filter("WebP", &["webp"])
                .add_filter("AVIF", &["avif"])
                .add_filter("TIFF", &["tif", "tiff"])
                .add_filter("BMP", &["bmp"])
                .add_filter("GIF", &["gif"])
                .add_filter("QOI", &["qoi"])
                .add_filter("TGA", &["tga"]);
            if let Some(dir) = dir {
                dlg = dlg.set_directory(dir);
            }
            let Some(handle) = dlg.save_file().await else {
                return;
            };
            let out = handle.path().to_path_buf();

            if let Some(w) = weak.upgrade() {
                w.set_loading(true);
            }
            thread::spawn(move || {
                if let Err(e) =
                    save_image(&base, &params, turns, &out)
                {
                    eprintln!("save failed: {e}");
                }
                let _ = weak
                    .upgrade_in_event_loop(|w| w.set_loading(false));
            });
        });
    });
}

pub fn image_viewer(
    path: impl AsRef<Path>,
) -> Result<(), slint::PlatformError> {
    let path = path.as_ref().to_path_buf();

    let w = ImageViewer::new()?;
    init_theme(&w);
    handle_properties(&w);

    let weak = w.as_weak();
    let tx = spawn_edit_worker(weak.clone());
    let (files, index) = list_siblings(&path);
    STATE.with_borrow_mut(|s| {
        *s = State {
            files,
            index,
            base: None,
            flip_h: false,
            flip_v: false,
            edit_tx: Some(tx),
        };
    });

    sync_nav(&w);
    wire(&w);
    load_current(&weak);
    w.show()?;

    IMAGE_WINDOW.with(|slot| *slot.borrow_mut() = Some(w));
    Ok(())
}
