use std::cell::RefCell;
use std::fs::{self, File, Metadata};
use std::io::BufReader;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Local};
use clipboard_rs::Clipboard;
use exif::{In, Tag, Value};
use lofty::prelude::*;
use slint::{Global, Weak};

use crate::explorer::path_to_url;
use crate::state::ui;
use crate::{
    App, AudioProperties, DirEntryProperties, ImageProperties,
    MediaKind, PropertiesBridge, VideoProperties,
};

thread_local! {
    static PATH: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub fn handle_properties(app: &App) {
    let b = PropertiesBridge::get(app);

    b.on_copy_path(|| {
        PATH.with_borrow(|p| {
            if let Some(p) = p {
                let url = path_to_url(p);
                println!("Copied {url}");
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
}

pub fn open_properties(weak: &Weak<App>, path: PathBuf) {
    let weak = weak.clone();

    let p = path.clone();

    let _ = weak.upgrade_in_event_loop(move |w| {
        let b = PropertiesBridge::get(&w);
        b.set_error("".into());
        b.set_loading(true);
        b.set_open(true);
        PATH.set(Some(p));
    });

    std::thread::spawn(move || {
        let result = gather(&path);

        let _ = weak.upgrade_in_event_loop(move |w| {
            let b = PropertiesBridge::get(&w);
            match result {
                Ok(info) => b.set_entry(info.into_slint()),
                Err(e) => b.set_error(
                    format!("Failed to open properties. {e}").into(),
                ),
            }
            b.set_loading(false);
        });
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
    item_count: String,
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
            item_count: self.item_count.into(),
            modified: self.modified.into(),
            created: self.created.into(),
            accessed: self.accessed.into(),
            permissions: self.permissions.into(),
            owner: self.owner.into(),
            group: self.group.into(),
            hidden: self.hidden,
            read_only: self.read_only,
            // TODO ?
            icon: slint::Image::default(),
            media: self.media,
            img: self.img,
            vid: self.vid,
            aud: self.aud,
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

    if is_dir {
        let stats = dir_stats(path, Duration::from_secs(2));
        let prefix = if stats.complete { "" } else { "≥ " };
        info.size = format!("{prefix}{}", human_size(stats.bytes));
        info.size_on_disk =
            format!("{prefix}{}", human_size(stats.disk_bytes));
        info.item_count = format!(
            "{} files, {} folders",
            stats.top_files, stats.top_dirs
        );
        info.media = MediaKind::Other;
    } else {
        info.size = human_size(meta.len());
        info.size_on_disk = human_size(meta.blocks() * 512);
        info.media = detect_kind(path);
        match info.media {
            MediaKind::Photo => info.img = image_props(path),
            MediaKind::Video => info.vid = video_props(path),
            MediaKind::Audio => info.aud = audio_props(path),
            MediaKind::Other => {}
        }
    }

    info.kind = kind_label(path, is_dir, is_symlink, info.media);
    Ok(info)
}

fn detect_kind(path: &Path) -> MediaKind {
    match mime_guess::from_path(path).first() {
        Some(m) => match m.type_().as_str() {
            "image" => MediaKind::Photo,
            "video" => MediaKind::Video,
            "audio" => MediaKind::Audio,
            _ => MediaKind::Other,
        },
        None => MediaKind::Other,
    }
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

struct DirStats {
    bytes: u64,
    disk_bytes: u64,
    top_files: usize,
    top_dirs: usize,
    complete: bool,
}

fn dir_stats(root: &Path, budget: Duration) -> DirStats {
    let start = Instant::now();
    let mut s = DirStats {
        bytes: 0,
        disk_bytes: 0,
        top_files: 0,
        top_dirs: 0,
        complete: true,
    };

    let mut stack: Vec<(PathBuf, bool)> =
        vec![(root.to_path_buf(), true)];

    while let Some((dir, is_top)) = stack.pop() {
        if start.elapsed() > budget {
            s.complete = false;
            break;
        }
        let Ok(rd) = fs::read_dir(&dir) else { continue };

        for entry in rd.flatten() {
            let Ok(m) = entry.metadata() else { continue };
            if m.is_dir() {
                if is_top {
                    s.top_dirs += 1;
                }
                stack.push((entry.path(), false));
            } else {
                if is_top {
                    s.top_files += 1;
                }
                s.bytes += m.len();
                s.disk_bytes += m.blocks() * 512;
            }
        }
    }
    s
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

fn image_props(path: &Path) -> ImageProperties {
    let mut p = ImageProperties::default();

    if let Ok(s) = imagesize::size(path) {
        let (w, h) = (s.width as u64, s.height as u64);
        p.dimensions = format!("{w} × {h}").into();
        p.megapixels =
            format!("{:.1} MP", (w * h) as f64 / 1e6).into();
        p.aspect_ratio = aspect(w, h).into();
    }

    let Ok(file) = File::open(path) else { return p };
    let Ok(exif) = exif::Reader::new()
        .read_from_container(&mut BufReader::new(file))
    else {
        return p;
    };

    let make = exif_ascii(&exif, Tag::Make);
    let model = exif_ascii(&exif, Tag::Model);
    let camera = if !make.is_empty() && model.starts_with(&make) {
        model
    } else {
        format!("{make} {model}").trim().to_string()
    };
    p.camera = camera.into();

    p.taken = exif_display(&exif, Tag::DateTimeOriginal).into();
    p.exposure = exif_display(&exif, Tag::ExposureTime).into();
    p.aperture = exif_display(&exif, Tag::FNumber).into();
    p.focal_length = exif_display(&exif, Tag::FocalLength).into();
    p.color_space = exif_display(&exif, Tag::ColorSpace).into();

    let iso = exif_display(&exif, Tag::PhotographicSensitivity);
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

    let lat = gps_coord(&exif, Tag::GPSLatitude, Tag::GPSLatitudeRef);
    let lon =
        gps_coord(&exif, Tag::GPSLongitude, Tag::GPSLongitudeRef);
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
    p
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
    }

    let props = tagged.properties();
    p.duration = fmt_duration(props.duration().as_secs()).into();
    p.codec =
        format!("{:?}", tagged.file_type()).to_uppercase().into();

    if let Some(b) = props.audio_bitrate() {
        p.bitrate = format!("{b} kbps").into();
    }
    if let Some(s) = props.sample_rate() {
        p.sample_rate = fmt_khz(s).into();
    }
    if let Some(c) = props.channels() {
        p.channels = fmt_channels(c as u64).into();
    }
    p
}

/// Video properties using ffprobe

fn parse_fps(s: &str) -> Option<f64> {
    let (n, d) = s.split_once('/')?;
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    (d != 0.0 && n != 0.0).then_some(n / d)
}

fn video_props(path: &Path) -> VideoProperties {
    let mut p = VideoProperties::default();

    p.no_ffprobe = true;

    let Ok(out) = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
    else {
        return p;
    };
    if !out.status.success() {
        return p;
    }
    let Ok(j) =
        serde_json::from_slice::<serde_json::Value>(&out.stdout)
    else {
        return p;
    };

    let streams =
        j["streams"].as_array().cloned().unwrap_or_default();
    let v = streams.iter().find(|s| s["codec_type"] == "video");
    let a = streams.iter().find(|s| s["codec_type"] == "audio");

    if let Some(v) = v {
        let (w, h) = (
            v["width"].as_u64().unwrap_or(0),
            v["height"].as_u64().unwrap_or(0),
        );
        if w > 0 && h > 0 {
            p.dimensions = format!("{w} × {h}").into();
        }

        p.aspect_ratio = match v["display_aspect_ratio"].as_str() {
            Some(r) if r != "0:1" && r != "N/A" => r.to_string(),
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

    let dur = j["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse::<f64>().ok())
        .or_else(|| {
            v.and_then(|v| v["duration"].as_str())
                .and_then(|d| d.parse::<f64>().ok())
        });
    if let Some(d) = dur {
        p.duration = fmt_duration(d as u64).into();
    }

    p.no_ffprobe = false;
    p
}
