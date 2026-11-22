use fast_image_resize::{
    FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer,
    images::{Image as FirImage, ImageRef},
};
use image::{DynamicImage, RgbaImage, imageops};
use rayon::prelude::*;
use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg,
};
use slint::{
    ComponentHandle, Global, Image, Rgba8Pixel, SharedPixelBuffer,
    Weak,
};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
};

use crate::properties::{handle_properties, open_properties};
use crate::{ImageViewer, PropertiesBridge, theme::init_theme};

thread_local! {
    static IMAGE_WINDOW: RefCell<Option<ImageViewer>> = RefCell::new(None);
    static STATE: RefCell<State> = RefCell::new(State::default());
}

static LOAD_GENERATION: AtomicU64 = AtomicU64::new(0);

const PREVIEW_MAX: u32 = 2048;
const MAX_TILE: u32 = 8192;
const DETAIL_PAD: f32 = 0.25;

pub const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "ico",
    "qoi", "tga", "dds", "exr", "hdr", "pbm", "pgm", "ppm", "pnm",
    "pam", "ff", "svg", "svgz",
];

#[derive(Default)]
struct State {
    files: Vec<PathBuf>,
    index: usize,
    base: Option<Arc<RgbaImage>>,
    vector: Option<Arc<usvg::Tree>>,
    preview: Option<Arc<RgbaImage>>,
    flip_h: bool,
    flip_v: bool,
    edit_tx: Option<mpsc::Sender<Job>>,
    detail_tx: Option<mpsc::Sender<DetailJob>>,
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

impl EditParams {
    fn is_identity(&self) -> bool {
        self.brightness == 0.0
            && self.contrast == 0.0
            && self.hue == 0.0
            && self.blur <= 0.01
            && self.sharpen <= 0.01
            && !self.invert
            && !self.flip_h
            && !self.flip_v
    }
}

struct Job {
    source: Arc<RgbaImage>,
    params: EditParams,
    scale: f32,
    generation: u64,
}

enum Source {
    Raster(Arc<RgbaImage>),
    Vector(Arc<usvg::Tree>),
}

struct DetailJob {
    source: Source,
    preview_ratio: f32,
    nx: f32,
    ny: f32,
    nw: f32,
    nh: f32,
    turns: i32,
    scale: f32,
    dpr: f32,
    epoch: u64,
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
    let raw: &mut [u8] = img;
    raw.par_chunks_exact_mut(4).for_each(|px| {
        for c in &mut px[..3] {
            *c = (*c as i32 + amount).clamp(0, 255) as u8;
        }
    });
}

fn invert_in_place(img: &mut RgbaImage) {
    let raw: &mut [u8] = img;
    raw.par_chunks_exact_mut(4).for_each(|px| {
        for c in &mut px[..3] {
            *c = 255 - *c;
        }
    });
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

fn render(
    base: &RgbaImage,
    p: &EditParams,
    turns: i32,
    scale: f32,
) -> RgbaImage {
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
        img = imageops::blur(&img, p.blur * scale);
    }
    if p.sharpen > 0.01 {
        img = imageops::unsharpen(&img, p.sharpen * 0.3 * scale, 0);
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
    let img = render(base, p, turns, 1.0);
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

fn resize_region(
    resizer: &mut Resizer,
    src: &RgbaImage,
    region: (u32, u32, u32, u32),
    out: (u32, u32),
    filter: FilterType,
) -> Option<RgbaImage> {
    let view = ImageRef::new(
        src.width(),
        src.height(),
        src.as_raw(),
        PixelType::U8x4,
    )
    .ok()?;
    let mut dst = FirImage::new(out.0, out.1, PixelType::U8x4);
    let alg = if region.2 == out.0 && region.3 == out.1 {
        ResizeAlg::Nearest
    } else {
        ResizeAlg::Convolution(filter)
    };
    let opts = ResizeOptions::new()
        .crop(
            region.0 as f64,
            region.1 as f64,
            region.2 as f64,
            region.3 as f64,
        )
        .resize_alg(alg);

    resizer.resize(&view, &mut dst, &opts).ok()?;
    RgbaImage::from_raw(out.0, out.1, dst.into_vec())
}

fn make_preview(base: &Arc<RgbaImage>) -> Arc<RgbaImage> {
    let (w, h) = base.dimensions();
    let longest = w.max(h);
    if longest <= PREVIEW_MAX {
        return base.clone();
    }
    let k = PREVIEW_MAX as f32 / longest as f32;
    let pw = ((w as f32 * k).round() as u32).max(1);
    let ph = ((h as f32 * k).round() as u32).max(1);
    let mut resizer = Resizer::new();
    match resize_region(
        &mut resizer,
        base,
        (0, 0, w, h),
        (pw, ph),
        FilterType::CatmullRom,
    ) {
        Some(p) => Arc::new(p),
        None => base.clone(),
    }
}

fn svg_options() -> usvg::Options<'static> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> =
        OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    });
    let mut opt = usvg::Options::default();
    opt.fontdb = fonts.clone();
    opt
}

fn rasterize_svg(
    tree: &usvg::Tree,
    rot: Transform,
    region: (f32, f32, f32, f32),
    out: (u32, u32),
) -> Option<RgbaImage> {
    const BAND: u32 = 256;
    const MARGIN: u32 = 32;

    let (ow, oh) = out;
    let ts = Transform::from_scale(
        ow as f32 / region.2,
        oh as f32 / region.3,
    )
    .pre_translate(-region.0, -region.1)
    .pre_concat(rot);

    let stride = ow as usize * 4;
    let mut raw = vec![0u8; stride * oh as usize];

    let ok = raw
        .par_chunks_mut(stride * BAND as usize)
        .enumerate()
        .all(|(i, chunk)| {
            let y0 = i as u32 * BAND;
            let rows = (chunk.len() / stride) as u32;
            let top = y0.saturating_sub(MARGIN);
            let bottom = (y0 + rows + MARGIN).min(oh);
            let Some(mut pixmap) = Pixmap::new(ow, bottom - top)
            else {
                return false;
            };
            resvg::render(
                tree,
                ts.post_translate(0.0, -(top as f32)),
                &mut pixmap.as_mut(),
            );
            let skip = (y0 - top) as usize * ow as usize;
            let src = &pixmap.pixels()
                [skip..skip + rows as usize * ow as usize];
            for (dst, p) in chunk.chunks_exact_mut(4).zip(src) {
                let c = p.demultiply();
                dst.copy_from_slice(&[
                    c.red(),
                    c.green(),
                    c.blue(),
                    c.alpha(),
                ]);
            }
            true
        });

    if !ok {
        return None;
    }
    RgbaImage::from_raw(ow, oh, raw)
}

fn load_svg(
    path: &Path,
) -> Result<(Arc<usvg::Tree>, Arc<RgbaImage>), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut opt = svg_options();
    opt.resources_dir = path.parent().map(Path::to_path_buf);
    let tree = usvg::Tree::from_data(&data, &opt)
        .map_err(|e| e.to_string())?;
    let size = tree.size();
    let (w, h) = (size.width(), size.height());
    let k = PREVIEW_MAX as f32 / w.max(h);
    let pw = ((w * k).round() as u32).max(1);
    let ph = ((h * k).round() as u32).max(1);
    let preview = rasterize_svg(
        &tree,
        Transform::identity(),
        (0.0, 0.0, w, h),
        (pw, ph),
    )
    .ok_or_else(|| "svg render failed".to_string())?;
    Ok((Arc::new(tree), Arc::new(preview)))
}

fn render_detail_raster(
    resizer: &mut Resizer,
    base: &RgbaImage,
    job: &DetailJob,
) -> Option<(SharedPixelBuffer<Rgba8Pixel>, [f32; 4])> {
    let (w, h) = base.dimensions();
    let turns = job.turns.rem_euclid(4);
    let (rw, rh) = if turns % 2 == 1 { (h, w) } else { (w, h) };

    let s = job.scale * job.dpr;
    if job.preview_ratio >= 1.0 || s <= job.preview_ratio {
        return None;
    }

    let x0 = ((job.nx - job.nw * DETAIL_PAD).max(0.0) * rw as f32)
        .floor() as u32;
    let y0 = ((job.ny - job.nh * DETAIL_PAD).max(0.0) * rh as f32)
        .floor() as u32;
    let x1 = (((job.nx + job.nw * (1.0 + DETAIL_PAD)).min(1.0))
        * rw as f32)
        .ceil() as u32;
    let y1 = (((job.ny + job.nh * (1.0 + DETAIL_PAD)).min(1.0))
        * rh as f32)
        .ceil() as u32;
    let (x1, y1) = (x1.min(rw), y1.min(rh));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (cw, ch) = (x1 - x0, y1 - y0);

    let (sx, sy, sw, sh) = match turns {
        0 => (x0, y0, cw, ch),
        1 => (y0, h - x1, ch, cw),
        2 => (w - x1, h - y1, cw, ch),
        _ => (w - y1, x0, ch, cw),
    };

    let k = s.min(1.0);
    let ow = ((cw as f32 * k).round() as u32).max(1);
    let oh = ((ch as f32 * k).round() as u32).max(1);
    let (dw, dh) = if turns % 2 == 1 { (oh, ow) } else { (ow, oh) };

    let tile = resize_region(
        resizer,
        base,
        (sx, sy, sw, sh),
        (dw, dh),
        FilterType::CatmullRom,
    )?;
    let tile = match turns {
        1 => imageops::rotate90(&tile),
        2 => imageops::rotate180(&tile),
        3 => imageops::rotate270(&tile),
        _ => tile,
    };

    Some((
        to_slint(&tile),
        [
            x0 as f32 / rw as f32,
            y0 as f32 / rh as f32,
            cw as f32 / rw as f32,
            ch as f32 / rh as f32,
        ],
    ))
}

fn render_detail_vector(
    tree: &usvg::Tree,
    job: &DetailJob,
) -> Option<(SharedPixelBuffer<Rgba8Pixel>, [f32; 4])> {
    let size = tree.size();
    let (w, h) = (size.width(), size.height());
    let turns = job.turns.rem_euclid(4);
    let (rw, rh) = if turns % 2 == 1 { (h, w) } else { (w, h) };

    let s = job.scale * job.dpr;
    if s <= job.preview_ratio {
        return None;
    }

    let nx0 = (job.nx - job.nw * DETAIL_PAD).max(0.0);
    let ny0 = (job.ny - job.nh * DETAIL_PAD).max(0.0);
    let nx1 = (job.nx + job.nw * (1.0 + DETAIL_PAD)).min(1.0);
    let ny1 = (job.ny + job.nh * (1.0 + DETAIL_PAD)).min(1.0);
    if nx1 <= nx0 || ny1 <= ny0 {
        return None;
    }

    let (x0, y0) = (nx0 * rw, ny0 * rh);
    let (cw, ch) = ((nx1 - nx0) * rw, (ny1 - ny0) * rh);
    let ow = ((cw * s).ceil() as u32).clamp(1, MAX_TILE);
    let oh = ((ch * s).ceil() as u32).clamp(1, MAX_TILE);

    let rot = match turns {
        1 => Transform::from_row(0.0, 1.0, -1.0, 0.0, h, 0.0),
        2 => Transform::from_row(-1.0, 0.0, 0.0, -1.0, w, h),
        3 => Transform::from_row(0.0, -1.0, 1.0, 0.0, 0.0, w),
        _ => Transform::identity(),
    };

    let tile = rasterize_svg(tree, rot, (x0, y0, cw, ch), (ow, oh))?;
    Some((to_slint(&tile), [nx0, ny0, nx1 - nx0, ny1 - ny0]))
}

fn render_detail(
    resizer: &mut Resizer,
    job: &DetailJob,
) -> Option<(SharedPixelBuffer<Rgba8Pixel>, [f32; 4])> {
    match &job.source {
        Source::Raster(base) => {
            render_detail_raster(resizer, base, job)
        }
        Source::Vector(tree) => render_detail_vector(tree, job),
    }
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

fn edits_active(w: &ImageViewer) -> bool {
    STATE.with_borrow(|s| {
        !read_params(w, s.flip_h, s.flip_v).is_identity()
    })
}

fn spawn_edit_worker(weak: Weak<ImageViewer>) -> mpsc::Sender<Job> {
    let (tx, rx) = mpsc::channel::<Job>();
    thread::spawn(move || {
        while let Ok(mut job) = rx.recv() {
            while let Ok(next) = rx.try_recv() {
                job = next;
            }
            let out = render(&job.source, &job.params, 0, job.scale);
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

fn spawn_detail_worker(
    weak: Weak<ImageViewer>,
) -> mpsc::Sender<DetailJob> {
    let (tx, rx) = mpsc::channel::<DetailJob>();
    thread::spawn(move || {
        let mut resizer = Resizer::new();
        while let Ok(mut job) = rx.recv() {
            while let Ok(next) = rx.try_recv() {
                job = next;
            }
            let out = render_detail(&mut resizer, &job);
            let (epoch, turns) = (job.epoch, job.turns);
            let res = weak.upgrade_in_event_loop(move |w| {
                if LOAD_GENERATION.load(Ordering::SeqCst) != epoch
                    || w.get_turns() != turns
                {
                    return;
                }
                if edits_active(&w) {
                    w.set_detail_valid(false);
                    return;
                }
                match out {
                    Some((buf, [x, y, dw, dh])) => {
                        w.set_detail(Image::from_rgba8(buf));
                        w.set_detail_x(x);
                        w.set_detail_y(y);
                        w.set_detail_w(dw);
                        w.set_detail_h(dh);
                        w.set_detail_valid(true);
                    }
                    None => w.set_detail_valid(false),
                }
            });
            if res.is_err() {
                break;
            }
        }
    });
    tx
}

fn request_detail(
    w: &ImageViewer,
    nx: f32,
    ny: f32,
    nw: f32,
    nh: f32,
    turns: i32,
    scale: f32,
) {
    let dpr = w.window().scale_factor();
    let job = STATE.with_borrow(|s| {
        let preview = s.preview.as_ref()?;
        let tx = s.detail_tx.as_ref()?;
        if !read_params(w, s.flip_h, s.flip_v).is_identity() {
            return None;
        }
        let (source, preview_ratio) = if let Some(tree) = &s.vector {
            (
                Source::Vector(tree.clone()),
                preview.width() as f32 / tree.size().width(),
            )
        } else {
            let base = s.base.as_ref()?;
            (
                Source::Raster(base.clone()),
                preview.width() as f32 / base.width() as f32,
            )
        };
        Some((
            tx.clone(),
            DetailJob {
                source,
                preview_ratio,
                nx,
                ny,
                nw,
                nh,
                turns,
                scale,
                dpr,
                epoch: LOAD_GENERATION.load(Ordering::SeqCst),
            },
        ))
    });
    match job {
        Some((tx, job)) => {
            let _ = tx.send(job);
        }
        None => w.set_detail_valid(false),
    }
}

fn install_base(
    w: &ImageViewer,
    base: Arc<RgbaImage>,
    preview: Arc<RgbaImage>,
) {
    LOAD_GENERATION.fetch_add(1, Ordering::SeqCst);
    w.set_full_width(base.width() as i32);
    w.set_full_height(base.height() as i32);
    w.set_detail_valid(false);
    STATE.with_borrow_mut(|s| {
        s.base = Some(base);
        s.vector = None;
        s.preview = Some(preview);
    });
}

fn install_vector(
    w: &ImageViewer,
    tree: Arc<usvg::Tree>,
    preview: Arc<RgbaImage>,
) {
    LOAD_GENERATION.fetch_add(1, Ordering::SeqCst);
    let size = tree.size();
    w.set_full_width((size.width().ceil() as i32).max(1));
    w.set_full_height((size.height().ceil() as i32).max(1));
    w.set_detail_valid(false);
    STATE.with_borrow_mut(|s| {
        s.base = None;
        s.vector = Some(tree);
        s.preview = Some(preview);
    });
}

fn run_edits(w: &ImageViewer) {
    STATE.with_borrow(|s| {
        if let (Some(base), Some(preview), Some(tx)) =
            (&s.base, &s.preview, &s.edit_tx)
        {
            let _ = tx.send(Job {
                source: preview.clone(),
                params: read_params(w, s.flip_h, s.flip_v),
                scale: preview.width() as f32 / base.width() as f32,
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
        let weak = weak.clone();
        thread::spawn(move || {
            let res = load_svg(&path).map(|(tree, preview)| {
                let buf = to_slint(&preview);
                (tree, preview, buf)
            });
            if LOAD_GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            let _ = weak.upgrade_in_event_loop(move |w| {
                if LOAD_GENERATION.load(Ordering::SeqCst)
                    != generation
                {
                    return;
                }
                match res {
                    Ok((tree, preview, buf)) => {
                        install_vector(&w, tree, preview);
                        w.set_can_edit(false);
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
        return;
    }

    let weak = weak.clone();
    thread::spawn(move || {
        let res = image::open(&path).map(|d| d.to_rgba8());
        if LOAD_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        let res = res.map(|img| {
            let base = Arc::new(img);
            let preview = make_preview(&base);
            let buf = to_slint(&preview);
            (base, preview, buf)
        });
        let _ = weak.upgrade_in_event_loop(move |w| {
            if LOAD_GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            match res {
                Ok((base, preview, buf)) => {
                    install_base(&w, base, preview);
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
                            IMAGE_EXTENSIONS.contains(
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
        s.vector = None;
        s.preview = None;
        s.flip_h = false;
        s.flip_v = false;
        true
    });
    if !moved {
        return;
    }
    let Some(w) = weak.upgrade() else { return };
    reset_edit_values(&w);
    w.set_detail_valid(false);
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
    w.on_detail_requested(move |nx, ny, nw, nh, turns, scale| {
        if let Some(w) = weak.upgrade() {
            request_detail(&w, nx, ny, nw, nh, turns, scale);
        }
    });

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

        let base = Arc::new(cropped);
        let preview = make_preview(&base);
        let buf = to_slint(&preview);

        STATE.with_borrow_mut(|s| {
            s.flip_h = false;
            s.flip_v = false;
        });
        install_base(&w, base, preview);
        w.set_turns(0);
        w.set_cropping(false);
        w.set_image(Image::from_rgba8(buf));
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
    let edit_tx = spawn_edit_worker(weak.clone());
    let detail_tx = spawn_detail_worker(weak.clone());
    let (files, index) = list_siblings(&path);
    STATE.with_borrow_mut(|s| {
        *s = State {
            files,
            index,
            edit_tx: Some(edit_tx),
            detail_tx: Some(detail_tx),
            ..State::default()
        };
    });

    sync_nav(&w);
    wire(&w);
    load_current(&weak);
    w.show()?;

    IMAGE_WINDOW.with(|slot| *slot.borrow_mut() = Some(w));
    Ok(())
}
