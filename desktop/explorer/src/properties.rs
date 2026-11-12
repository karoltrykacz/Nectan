use chrono::{DateTime, Local};
use clipboard_rs::Clipboard;
use exif::{In, Tag, Value};
use ignore::{WalkBuilder, WalkState};
use lofty::file::FileType;
use lofty::prelude::*;
use lofty::tag::{ItemKey, Tag as AudioTag, TagItem};
use slint::{Global, Weak};
use std::cell::RefCell;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, ErrorKind, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime};

use crate::explorer::path_to_url;
use crate::state::ui;
use crate::{
    App, AudioProperties, DirEntryProperties, DirProperties,
    ImageProperties, MediaKind, PropertiesBridge, TextProperties,
    VideoProperties,
};

thread_local! {
    static PATH: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

static CANCEL_SCAN: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

pub fn abort_walk() {
    if let Some(c) = CANCEL_SCAN.lock().unwrap().take() {
        c.store(true, Ordering::Relaxed);
    }
}

pub fn handle_properties(app: &App) {
    let b = PropertiesBridge::get(app);

    b.on_copy_path(|| {
        PATH.with_borrow(|p| {
            if let Some(p) = p {
                let url = path_to_url(p);
                ui().clipboard().set_text(url);
            }
        });
    });

    b.on_open_in_maps(|lat, lon| {
        let url = format!(
            "https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=16/{lat}/{lon}"
        );
        let _ = open::that_detached(url);
    });

    b.on_closed(abort_walk);
}

pub fn open_properties(weak: &Weak<App>, path: PathBuf) {
    abort_walk();
    let cancel = Arc::new(AtomicBool::new(false));
    *CANCEL_SCAN.lock().unwrap() = Some(cancel.clone());

    let weak = weak.clone();
    let p = path.clone();

    let _ = weak.upgrade_in_event_loop(move |w| {
        let b = PropertiesBridge::get(&w);
        b.set_error("".into());
        b.set_folder(DirProperties::default());
        b.set_loading(true);
        b.set_open(true);
        PATH.set(Some(p));
    });

    std::thread::spawn(move || {
        let result = gather(&path);
        let is_dir = matches!(&result, Ok(i) if i.is_dir);
        let c = cancel.clone();

        let _ = weak.upgrade_in_event_loop(move |w| {
            if c.load(Ordering::Relaxed) {
                return;
            }
            let b = PropertiesBridge::get(&w);
            match result {
                Ok(info) => {
                    if info.is_dir {
                        b.set_folder(
                            Snapshot::started().into_slint(),
                        );
                    }
                    b.set_entry(info.into_slint());
                }
                Err(e) => b.set_error(
                    format!("Failed to open properties. {e}").into(),
                ),
            }
            b.set_loading(false);
        });

        if is_dir {
            scan_folder(&weak, &path, &cancel);
        }
    });
}

#[derive(Default)]
struct Info {
    name: String,
    path: String,
    location: String,
    kind: String,
    is_dir: bool,
    is_symlink: bool,
    link_target: String,
    size: String,
    size_on_disk: String,
    modified: String,
    created: String,
    accessed: String,
    permissions: String,
    owner: String,
    group: String,
    hidden: bool,
    read_only: bool,
    media: MediaKind,
    img: ImageProperties,
    vid: VideoProperties,
    aud: AudioProperties,
    txt: TextProperties,
}

impl Info {
    fn into_slint(self) -> DirEntryProperties {
        DirEntryProperties {
            name: self.name.into(),
            path: self.path.into(),
            location: self.location.into(),
            kind: self.kind.into(),
            is_dir: self.is_dir,
            is_symlink: self.is_symlink,
            link_target: self.link_target.into(),
            size: self.size.into(),
            size_on_disk: self.size_on_disk.into(),
            modified: self.modified.into(),
            created: self.created.into(),
            accessed: self.accessed.into(),
            permissions: self.permissions.into(),
            owner: self.owner.into(),
            group: self.group.into(),
            hidden: self.hidden,
            read_only: self.read_only,
            icon: slint::Image::default(),
            media: self.media,
            img: self.img,
            vid: self.vid,
            aud: self.aud,
            txt: self.txt,
        }
    }
}

fn gather(path: &Path) -> Result<Info, String> {
    let lmeta =
        fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let is_symlink = lmeta.file_type().is_symlink();
    let meta = fs::metadata(path).unwrap_or_else(|_| lmeta.clone());
    let is_dir = meta.is_dir();

    let mut info = Info {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
        path: path.display().to_string(),
        location: path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        is_dir,
        is_symlink,
        link_target: if is_symlink {
            fs::read_link(path)
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        } else {
            String::new()
        },
        modified: fmt_time(meta.modified()),
        created: fmt_time(meta.created()),
        accessed: fmt_time(meta.accessed()),
        permissions: fmt_permissions(meta.mode()),
        owner: uzers::get_user_by_uid(meta.uid())
            .map(|u| u.name().to_string_lossy().into_owned())
            .unwrap_or_else(|| meta.uid().to_string()),
        group: uzers::get_group_by_gid(meta.gid())
            .map(|g| g.name().to_string_lossy().into_owned())
            .unwrap_or_else(|| meta.gid().to_string()),
        read_only: meta.permissions().readonly(),
        ..Default::default()
    };
    info.hidden = info.name.starts_with('.');

    if !is_dir {
        info.size = human_size(meta.len());
        info.size_on_disk = human_size(meta.blocks() * 512);

        if meta.is_file() {
            info.media = match detect_kind(path) {
                MediaKind::Other if sniff_text(path) => {
                    MediaKind::Text
                }
                k => k,
            };
            match info.media {
                MediaKind::Photo if !is_svg(path) => {
                    info.img = image_props(path)
                }
                MediaKind::Video => info.vid = video_props(path),
                MediaKind::Audio => info.aud = audio_props(path),
                MediaKind::Text => info.txt = text_props(path),
                MediaKind::Photo | MediaKind::Other => {}
            }
        }
    }

    info.kind = kind_label(path, is_dir, is_symlink, info.media);
    Ok(info)
}

fn is_svg(path: &Path) -> bool {
    path.extension().is_some_and(|e| {
        e.eq_ignore_ascii_case("svg")
            || e.eq_ignore_ascii_case("svgz")
    })
}

fn detect_kind(path: &Path) -> MediaKind {
    match mime_guess::from_path(path).first() {
        Some(m) => match (m.type_().as_str(), m.subtype().as_str()) {
            ("image", _) => MediaKind::Photo,
            ("video", _) => MediaKind::Video,
            ("audio", _) => MediaKind::Audio,
            ("text", _) => MediaKind::Text,
            (
                "application",
                "json" | "xml" | "toml" | "yaml" | "x-yaml"
                | "javascript" | "x-sh" | "sql",
            ) => MediaKind::Text,
            _ => MediaKind::Other,
        },
        None => MediaKind::Other,
    }
}

fn looks_text(buf: &[u8]) -> bool {
    if buf.is_empty() {
        return false;
    }
    if buf.starts_with(&[0xFF, 0xFE])
        || buf.starts_with(&[0xFE, 0xFF])
    {
        return true;
    }
    if buf.contains(&0) {
        return false;
    }
    match std::str::from_utf8(buf) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none(),
    }
}

fn sniff_text(path: &Path) -> bool {
    let Ok(f) = File::open(path) else {
        return false;
    };
    let mut buf = Vec::new();
    if f.take(8192).read_to_end(&mut buf).is_err() {
        return false;
    }
    looks_text(&buf)
}

fn kind_label(
    path: &Path,
    is_dir: bool,
    is_symlink: bool,
    media: MediaKind,
) -> String {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_uppercase())
        .unwrap_or_default();

    let base = if is_dir {
        "Folder".to_string()
    } else {
        match (media, ext.is_empty()) {
            (MediaKind::Photo, false) => format!("{ext} image"),
            (MediaKind::Video, false) => format!("{ext} video"),
            (MediaKind::Audio, false) => format!("{ext} audio"),
            (MediaKind::Text, false) => format!("{ext} text file"),
            (MediaKind::Text, true) => "Text file".to_string(),
            (_, false) => format!("{ext} file"),
            (_, true) => "File".to_string(),
        }
    };

    if is_symlink {
        format!("Link to {}", base.to_lowercase())
    } else {
        base
    }
}

#[derive(Default)]
struct Stats {
    bytes: AtomicU64,
    disk: AtomicU64,
    files: AtomicU64,
    dirs: AtomicU64,
    hidden: AtomicU64,
    links: AtomicU64,
    top_files: AtomicU64,
    top_dirs: AtomicU64,
    seen: Mutex<HashSet<(u64, u64)>>,
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Snapshot {
    bytes: u64,
    disk: u64,
    files: u64,
    dirs: u64,
    hidden: u64,
    links: u64,
    top_files: u64,
    top_dirs: u64,
    scanning: bool,
}

impl Snapshot {
    fn started() -> Self {
        Self {
            scanning: true,
            ..Default::default()
        }
    }

    fn into_slint(self) -> DirProperties {
        let pre = if self.scanning { "≥ " } else { "" };
        DirProperties {
            scanning: self.scanning,
            size: format!("{pre}{}", human_size(self.bytes)).into(),
            size_on_disk: format!("{pre}{}", human_size(self.disk))
                .into(),
            items: format!(
                "{} files, {} folders",
                self.top_files, self.top_dirs
            )
            .into(),
            files: self.files.to_string().into(),
            folders: self.dirs.to_string().into(),
            hidden: self.hidden.to_string().into(),
            links: self.links.to_string().into(),
        }
    }
}

impl Stats {
    fn snapshot(&self, scanning: bool) -> Snapshot {
        let r = Ordering::Relaxed;
        Snapshot {
            bytes: self.bytes.load(r),
            disk: self.disk.load(r),
            files: self.files.load(r),
            dirs: self.dirs.load(r),
            hidden: self.hidden.load(r),
            links: self.links.load(r),
            top_files: self.top_files.load(r),
            top_dirs: self.top_dirs.load(r),
            scanning,
        }
    }

    fn record(&self, entry: &ignore::DirEntry) {
        let depth = entry.depth();
        if depth == 0 {
            return;
        }
        let Ok(m) = entry.metadata() else { return };
        let r = Ordering::Relaxed;
        let top = depth == 1;

        if entry.file_name().as_bytes().first() == Some(&b'.') {
            self.hidden.fetch_add(1, r);
        }

        if m.is_dir() {
            self.dirs.fetch_add(1, r);
            if top {
                self.top_dirs.fetch_add(1, r);
            }
            return;
        }

        self.files.fetch_add(1, r);
        if top {
            self.top_files.fetch_add(1, r);
        }
        if m.file_type().is_symlink() {
            self.links.fetch_add(1, r);
        }

        let unique = m.nlink() < 2
            || self.seen.lock().unwrap().insert((m.dev(), m.ino()));
        if unique {
            self.bytes.fetch_add(m.len(), r);
            self.disk.fetch_add(m.blocks() * 512, r);
        }
    }
}

fn walk(root: &Path, stats: &Stats, cancel: &AtomicBool) {
    WalkBuilder::new(root)
        .standard_filters(false)
        .follow_links(false)
        .build_parallel()
        .run(move || {
            Box::new(move |res| {
                if cancel.load(Ordering::Relaxed) {
                    return WalkState::Quit;
                }
                if let Ok(entry) = res {
                    stats.record(&entry);
                }
                WalkState::Continue
            })
        });
}

fn scan_folder(
    weak: &Weak<App>,
    root: &Path,
    cancel: &Arc<AtomicBool>,
) {
    let stats = Arc::new(Stats::default());
    let done = Arc::new(AtomicBool::new(false));

    {
        let stats = stats.clone();
        let done = done.clone();
        let cancel = cancel.clone();
        let root = fs::canonicalize(root)
            .unwrap_or_else(|_| root.to_path_buf());
        std::thread::spawn(move || {
            walk(&root, &stats, &cancel);
            done.store(true, Ordering::Release);
        });
    }

    let mut last: Option<Snapshot> = None;
    loop {
        std::thread::sleep(Duration::from_millis(16));
        if cancel.load(Ordering::Relaxed) {
            return;
        }

        let finished = done.load(Ordering::Acquire);
        let snap = stats.snapshot(!finished);

        if last != Some(snap) {
            last = Some(snap);
            let c = cancel.clone();
            let _ = weak.upgrade_in_event_loop(move |w| {
                if !c.load(Ordering::Relaxed) {
                    PropertiesBridge::get(&w)
                        .set_folder(snap.into_slint());
                }
            });
        }

        if finished {
            return;
        }
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", UNITS[i - 1])
}

fn fmt_time(t: std::io::Result<SystemTime>) -> String {
    match t {
        Ok(t) => DateTime::<Local>::from(t)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        Err(_) => "—".to_string(),
    }
}

fn fmt_permissions(mode: u32) -> String {
    let s: String = (0..9)
        .map(|i| {
            let bit = 1 << (8 - i);
            if mode & bit == 0 {
                '-'
            } else {
                ['r', 'w', 'x'][i % 3]
            }
        })
        .collect();
    format!("{s} ({:04o})", mode & 0o7777)
}

fn fmt_duration(secs: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

fn fmt_khz(hz: u32) -> String {
    if hz % 1000 == 0 {
        format!("{} kHz", hz / 1000)
    } else {
        format!("{:.1} kHz", hz as f64 / 1000.0)
    }
}

fn fmt_channels(n: u64) -> String {
    match n {
        1 => "Mono".into(),
        2 => "Stereo".into(),
        n => format!("{n} channels"),
    }
}

fn fmt_bitrate_bps(bps: u64) -> String {
    if bps >= 1_000_000 {
        format!("{:.1} Mbps", bps as f64 / 1e6)
    } else {
        format!("{} kbps", bps / 1000)
    }
}

fn fmt_pair(n: Option<u32>, total: Option<u32>) -> String {
    match (n, total) {
        (Some(n), Some(t)) => format!("{n} / {t}"),
        (Some(n), None) => n.to_string(),
        _ => String::new(),
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn aspect(w: u64, h: u64) -> String {
    if w == 0 || h == 0 {
        return String::new();
    }
    let g = gcd(w, h).max(1);
    format!("{}:{}", w / g, h / g)
}

fn filled(fields: &[&slint::SharedString]) -> bool {
    fields.iter().any(|s| !s.is_empty())
}

fn exif_ascii(exif: &exif::Exif, tag: Tag) -> String {
    match exif.get_field(tag, In::PRIMARY).map(|f| &f.value) {
        Some(Value::Ascii(v)) => v
            .iter()
            .map(|b| String::from_utf8_lossy(b).trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

fn exif_display(exif: &exif::Exif, tag: Tag) -> String {
    exif.get_field(tag, In::PRIMARY)
        .map(|f| f.display_value().with_unit(exif).to_string())
        .unwrap_or_default()
}

fn exif_orientation(exif: &exif::Exif) -> Option<u32> {
    exif.get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
}

fn orientation_label(o: u32) -> &'static str {
    match o {
        1 => "Normal",
        2 => "Mirrored",
        3 => "Rotated 180°",
        4 => "Mirrored vertically",
        5 => "Mirrored, rotated 270° CW",
        6 => "Rotated 90° CW",
        7 => "Mirrored, rotated 90° CW",
        8 => "Rotated 270° CW",
        _ => "",
    }
}

fn gps_coord(
    exif: &exif::Exif,
    tag: Tag,
    ref_tag: Tag,
) -> Option<f64> {
    let f = exif.get_field(tag, In::PRIMARY)?;
    let Value::Rational(v) = &f.value else {
        return None;
    };
    if v.len() < 3 {
        return None;
    }
    let deg =
        v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0;
    let r = exif_ascii(exif, ref_tag);
    Some(if r.starts_with('S') || r.starts_with('W') {
        -deg
    } else {
        deg
    })
}

fn gps_altitude(exif: &exif::Exif) -> String {
    let Some(f) = exif.get_field(Tag::GPSAltitude, In::PRIMARY)
    else {
        return String::new();
    };
    let Value::Rational(v) = &f.value else {
        return String::new();
    };
    let Some(r) = v.first() else {
        return String::new();
    };
    let below = exif
        .get_field(Tag::GPSAltitudeRef, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        == Some(1);
    format!(
        "{:.1} m{}",
        r.to_f64(),
        if below { " below sea level" } else { "" }
    )
}

fn fill_exif(p: &mut ImageProperties, exif: &exif::Exif) {
    let make = exif_ascii(exif, Tag::Make);
    let model = exif_ascii(exif, Tag::Model);
    let camera = if !make.is_empty() && model.starts_with(&make) {
        model
    } else {
        format!("{make} {model}").trim().to_string()
    };
    p.camera = camera.into();
    p.lens = exif_ascii(exif, Tag::LensModel).into();

    p.taken = exif_display(exif, Tag::DateTimeOriginal).into();
    p.exposure = exif_display(exif, Tag::ExposureTime).into();
    p.aperture = exif_display(exif, Tag::FNumber).into();
    p.focal_length = exif_display(exif, Tag::FocalLength).into();
    p.focal_length_35 =
        exif_display(exif, Tag::FocalLengthIn35mmFilm).into();
    p.exposure_bias =
        exif_display(exif, Tag::ExposureBiasValue).into();
    p.exposure_program =
        exif_display(exif, Tag::ExposureProgram).into();
    p.metering = exif_display(exif, Tag::MeteringMode).into();
    p.white_balance = exif_display(exif, Tag::WhiteBalance).into();
    p.flash = exif_display(exif, Tag::Flash).into();

    p.color_space = exif_display(exif, Tag::ColorSpace).into();
    p.resolution = exif_display(exif, Tag::XResolution).into();
    if let Some(o) = exif_orientation(exif) {
        p.orientation = orientation_label(o).into();
    }

    p.software = exif_ascii(exif, Tag::Software).into();
    p.artist = exif_ascii(exif, Tag::Artist).into();
    p.copyright = exif_ascii(exif, Tag::Copyright).into();

    let iso = exif_display(exif, Tag::PhotographicSensitivity);
    if !iso.is_empty() {
        p.iso = format!("ISO {iso}").into();
    }

    if let Some(Value::Short(v)) = exif
        .get_field(Tag::BitsPerSample, In::PRIMARY)
        .map(|f| &f.value)
    {
        if let Some(b) = v.first() {
            p.bit_depth = format!("{b}-bit").into();
        }
    }

    let lat = gps_coord(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef);
    let lon =
        gps_coord(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef);
    if let (Some(lat), Some(lon)) = (lat, lon) {
        p.gps = format!(
            "{:.4}° {}, {:.4}° {}",
            lat.abs(),
            if lat >= 0.0 { "N" } else { "S" },
            lon.abs(),
            if lon >= 0.0 { "E" } else { "W" },
        )
        .into();
        p.gps_lat = lat as f32;
        p.gps_lon = lon as f32;
    }
    p.altitude = gps_altitude(exif).into();
}

fn image_props(path: &Path) -> ImageProperties {
    let mut p = ImageProperties::default();

    let exif = File::open(path).ok().and_then(|f| {
        exif::Reader::new()
            .read_from_container(&mut BufReader::new(f))
            .ok()
    });

    let orientation =
        exif.as_ref().and_then(exif_orientation).unwrap_or(1);

    if let Ok(s) = imagesize::size(path) {
        let (mut w, mut h) = (s.width as u64, s.height as u64);
        if (5..=8).contains(&orientation) {
            std::mem::swap(&mut w, &mut h);
        }
        p.dimensions = format!("{w} × {h}").into();
        p.megapixels =
            format!("{:.1} MP", (w * h) as f64 / 1e6).into();
        p.aspect_ratio = aspect(w, h).into();
    }

    if let Some(exif) = &exif {
        fill_exif(&mut p, exif);
    }

    p.has_image = filled(&[
        &p.dimensions,
        &p.megapixels,
        &p.aspect_ratio,
        &p.color_space,
        &p.bit_depth,
        &p.orientation,
        &p.resolution,
    ]);
    p.has_camera = filled(&[
        &p.camera,
        &p.lens,
        &p.taken,
        &p.exposure,
        &p.aperture,
        &p.iso,
        &p.focal_length,
        &p.focal_length_35,
        &p.exposure_bias,
        &p.exposure_program,
        &p.metering,
        &p.white_balance,
        &p.flash,
    ]);
    p.has_location = filled(&[&p.gps, &p.altitude]);
    p.has_details = filled(&[&p.software, &p.artist, &p.copyright]);
    p
}

fn find_text(
    tag: &AudioTag,
    pred: impl Fn(&TagItem) -> bool,
) -> String {
    tag.items()
        .find(|i| pred(*i))
        .and_then(|i| i.value().text())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn audio_props(path: &Path) -> AudioProperties {
    let mut p = AudioProperties::default();
    let Ok(tagged) = lofty::read_from_path(path) else {
        return p;
    };

    if let Some(tag) =
        tagged.primary_tag().or_else(|| tagged.first_tag())
    {
        p.title = tag
            .title()
            .map(|s| s.into_owned())
            .unwrap_or_default()
            .into();
        p.artist = tag
            .artist()
            .map(|s| s.into_owned())
            .unwrap_or_default()
            .into();
        p.album = tag
            .album()
            .map(|s| s.into_owned())
            .unwrap_or_default()
            .into();
        p.genre = tag
            .genre()
            .map(|s| s.into_owned())
            .unwrap_or_default()
            .into();
        p.album_artist = find_text(tag, |i| {
            matches!(i.key(), ItemKey::AlbumArtist)
        })
        .into();
        p.composer =
            find_text(tag, |i| matches!(i.key(), ItemKey::Composer))
                .into();
        p.year = find_text(tag, |i| {
            matches!(i.key(), ItemKey::RecordingDate)
        })
        .chars()
        .take(4)
        .collect::<String>()
        .into();
        p.track = fmt_pair(tag.track(), tag.track_total()).into();
        p.disc = fmt_pair(tag.disk(), tag.disk_total()).into();
        if !tag.pictures().is_empty() {
            p.cover = "Yes".into();
        }
    }

    let props = tagged.properties();
    p.duration = fmt_duration(props.duration().as_secs()).into();
    p.codec =
        format!("{:?}", tagged.file_type()).to_uppercase().into();
    p.quality = match tagged.file_type() {
        FileType::Flac
        | FileType::Wav
        | FileType::Aiff
        | FileType::Ape
        | FileType::WavPack => "Lossless",
        FileType::Mpeg
        | FileType::Opus
        | FileType::Vorbis
        | FileType::Speex => "Lossy",
        _ => "",
    }
    .into();

    if let Some(b) = props.audio_bitrate() {
        p.bitrate = format!("{b} kbps").into();
    }
    if let Some(s) = props.sample_rate() {
        p.sample_rate = fmt_khz(s).into();
    }
    if let Some(c) = props.channels() {
        p.channels = fmt_channels(c as u64).into();
    }
    if let Some(d) = props.bit_depth() {
        p.bit_depth = format!("{d}-bit").into();
    }
    p
}

fn parse_fps(s: &str) -> Option<f64> {
    let (n, d) = s.split_once('/')?;
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    (d != 0.0 && n != 0.0).then_some(n / d)
}

fn tag_get(v: &serde_json::Value, key: &str) -> String {
    v["tags"]
        .as_object()
        .and_then(|m| {
            m.iter().find(|(k, _)| k.eq_ignore_ascii_case(key))
        })
        .and_then(|(_, v)| v.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn known(v: &serde_json::Value, key: &str) -> Option<String> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty() && *s != "unknown")
        .map(str::to_string)
}

fn streams_of<'a>(
    streams: &'a [serde_json::Value],
    kind: &str,
) -> Vec<&'a serde_json::Value> {
    streams.iter().filter(|s| s["codec_type"] == kind).collect()
}

fn track_summary(streams: &[&serde_json::Value]) -> String {
    if streams.is_empty() {
        return String::new();
    }
    let mut langs: Vec<String> = Vec::new();
    for s in streams {
        let l = tag_get(s, "language");
        if !l.is_empty() && l != "und" && !langs.contains(&l) {
            langs.push(l);
        }
    }
    if langs.is_empty() {
        streams.len().to_string()
    } else {
        format!("{} ({})", streams.len(), langs.join(", "))
    }
}

fn rotation(v: &serde_json::Value) -> i64 {
    let r = v["side_data_list"]
        .as_array()
        .and_then(|l| l.iter().find_map(|d| d["rotation"].as_f64()))
        .or_else(|| tag_get(v, "rotate").parse::<f64>().ok())
        .unwrap_or(0.0);
    (r.round() as i64).rem_euclid(360)
}

fn pix_depth(fmt: &str) -> Option<u8> {
    let f =
        fmt.strip_suffix("le").or_else(|| fmt.strip_suffix("be"))?;
    [9u8, 10, 12, 14, 16]
        .into_iter()
        .find(|d| f.ends_with(&d.to_string()))
}

fn ffprobe(
    path: &Path,
    missing: &mut bool,
) -> Option<serde_json::Value> {
    let mut child = match Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            *missing = e.kind() == ErrorKind::NotFound;
            return None;
        }
    };

    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });

    let Ok(buf) = rx.recv_timeout(Duration::from_secs(8)) else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    if !child.wait().ok()?.success() {
        return None;
    }
    serde_json::from_slice(&buf).ok()
}

fn video_props(path: &Path) -> VideoProperties {
    let mut p = VideoProperties::default();

    let mut missing = false;
    let Some(j) = ffprobe(path, &mut missing) else {
        p.no_ffprobe = missing;
        return p;
    };

    let streams: &[serde_json::Value] =
        j["streams"].as_array().map(|v| v.as_slice()).unwrap_or(&[]);
    let videos = streams_of(streams, "video");
    let audios = streams_of(streams, "audio");
    let subs = streams_of(streams, "subtitle");
    let v = videos.iter().copied().find(|s| {
        s["disposition"]["attached_pic"].as_i64() != Some(1)
    });
    let a = audios.first().copied();

    let fmt = &j["format"];
    p.container =
        fmt["format_long_name"].as_str().unwrap_or("").into();
    if let Some(b) =
        fmt["bit_rate"].as_str().and_then(|b| b.parse::<u64>().ok())
    {
        p.bitrate = fmt_bitrate_bps(b).into();
    }
    p.title = tag_get(fmt, "title").into();
    let created = tag_get(fmt, "creation_time");
    p.created = DateTime::parse_from_rfc3339(&created)
        .map(|d| {
            d.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or(created)
        .into();

    if let Some(v) = v {
        let (mut w, mut h) = (
            v["width"].as_u64().unwrap_or(0),
            v["height"].as_u64().unwrap_or(0),
        );
        let rot = rotation(v);
        let rotated = rot % 180 == 90;
        if rotated {
            std::mem::swap(&mut w, &mut h);
        }
        if w > 0 && h > 0 {
            p.dimensions = format!("{w} × {h}").into();
        }
        if rot != 0 {
            p.rotation = format!("{rot}°").into();
        }

        p.aspect_ratio = match v["display_aspect_ratio"].as_str() {
            Some(r) if !rotated && r != "0:1" && r != "N/A" => {
                r.to_string()
            }
            _ => aspect(w, h),
        }
        .into();

        let codec =
            v["codec_name"].as_str().unwrap_or("").to_uppercase();
        p.video_codec = match v["profile"].as_str() {
            Some(prof) if !codec.is_empty() => {
                format!("{codec} ({prof})")
            }
            _ => codec,
        }
        .into();

        let fps =
            v["avg_frame_rate"].as_str().and_then(parse_fps).or_else(
                || v["r_frame_rate"].as_str().and_then(parse_fps),
            );
        if let Some(r) = fps {
            p.frame_rate = format!("{r:.2} fps").into();
        }

        if let Some(b) =
            v["bit_rate"].as_str().and_then(|b| b.parse::<u64>().ok())
        {
            p.video_bitrate = fmt_bitrate_bps(b).into();
        }

        p.pixel_format = v["pix_fmt"].as_str().unwrap_or("").into();
        let depth = v["bits_per_raw_sample"]
            .as_str()
            .and_then(|b| b.parse::<u8>().ok())
            .or_else(|| v["pix_fmt"].as_str().and_then(pix_depth));
        if let Some(d) = depth {
            p.bit_depth = format!("{d}-bit").into();
        }

        let mut color = Vec::new();
        if let Some(s) = known(v, "color_space") {
            color.push(s);
        }
        if let Some(r) = known(v, "color_range") {
            color.push(
                match r.as_str() {
                    "tv" => "limited",
                    "pc" => "full",
                    o => o,
                }
                .to_string(),
            );
        }
        p.color = color.join(", ").into();

        p.hdr = match v["color_transfer"].as_str() {
            Some("smpte2084") => "HDR10 (PQ)",
            Some("arib-std-b67") => "HLG",
            _ => "",
        }
        .into();
    }

    if let Some(a) = a {
        p.audio_codec = a["codec_name"]
            .as_str()
            .unwrap_or("")
            .to_uppercase()
            .into();

        let mut parts = Vec::new();
        if let Some(ch) = a["channels"].as_u64() {
            parts.push(fmt_channels(ch));
        }
        if let Some(sr) = a["sample_rate"]
            .as_str()
            .and_then(|s| s.parse::<u32>().ok())
        {
            parts.push(fmt_khz(sr));
        }
        if let Some(b) =
            a["bit_rate"].as_str().and_then(|b| b.parse::<u64>().ok())
        {
            parts.push(fmt_bitrate_bps(b));
        }
        p.audio_info = parts.join(", ").into();
    }

    p.audio_tracks = track_summary(&audios).into();
    p.subtitles = track_summary(&subs).into();

    let dur = fmt["duration"]
        .as_str()
        .and_then(|d| d.parse::<f64>().ok())
        .or_else(|| {
            v.and_then(|v| v["duration"].as_str())
                .and_then(|d| d.parse::<f64>().ok())
        });
    if let Some(d) = dur {
        p.duration = fmt_duration(d as u64).into();
    }

    p
}

fn text_props(path: &Path) -> TextProperties {
    const LIMIT: u64 = 128 * 1024 * 1024;

    let mut p = TextProperties::default();
    let Ok(mut f) = File::open(path) else {
        return p;
    };

    let mut buf = vec![0u8; 64 * 1024];
    let (mut total, mut lf, mut crlf, mut cr) =
        (0u64, 0u64, 0u64, 0u64);
    let (mut ascii, mut prev) = (true, 0u8);
    let mut head = [0u8; 3];

    while total < LIMIT {
        let Ok(n) = f.read(&mut buf) else { break };
        if n == 0 {
            break;
        }
        if total == 0 {
            let k = n.min(3);
            head[..k].copy_from_slice(&buf[..k]);
        }
        for &b in &buf[..n] {
            match b {
                b'\n' if prev == b'\r' => {
                    crlf += 1;
                    cr -= 1;
                }
                b'\n' => lf += 1,
                b'\r' => cr += 1,
                0x80..=0xFF => ascii = false,
                _ => {}
            }
            prev = b;
        }
        total += n as u64;
    }

    let utf16 =
        head[..2] == [0xFF, 0xFE] || head[..2] == [0xFE, 0xFF];
    p.encoding = if head == [0xEF, 0xBB, 0xBF] {
        "UTF-8 with BOM"
    } else if head[..2] == [0xFF, 0xFE] {
        "UTF-16 LE"
    } else if head[..2] == [0xFE, 0xFF] {
        "UTF-16 BE"
    } else if ascii {
        "ASCII"
    } else {
        "UTF-8"
    }
    .into();

    if utf16 {
        return p;
    }

    let truncated = total >= LIMIT;
    let pre = if truncated { "≥ " } else { "" };
    let lines = lf
        + crlf
        + cr
        + u64::from(total > 0 && !matches!(prev, b'\n' | b'\r'));
    p.lines = format!("{pre}{lines}").into();

    let kinds: Vec<&str> = [(lf, "LF"), (crlf, "CRLF"), (cr, "CR")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(_, s)| s)
        .collect();
    p.line_endings = match kinds.as_slice() {
        [] => "",
        ["LF"] => "LF (Unix)",
        ["CRLF"] => "CRLF (Windows)",
        ["CR"] => "CR (Classic Mac)",
        _ => "Mixed",
    }
    .into();
    p
}
