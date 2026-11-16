use lofty::prelude::*;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};
use slint::{
    ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, Timer,
    TimerMode,
};
use std::cell::RefCell;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{MusicPlayer, theme::init_theme};

const EXTENSIONS: &[&str] = &["mp3", "flac", "ogg", "wav"];

thread_local! {
    static PLAYER_WINDOW: RefCell<Option<(MusicPlayer, Timer)>> = RefCell::new(None);
}

struct AudioPlayer {
    _stream: MixerDeviceSink,
    sink: Player,
    queue: Vec<PathBuf>,
    index: usize,
    duration: f32,
    default_art: Image,
}
fn shutdown(p: &Rc<RefCell<AudioPlayer>>) {
    {
        let mut p = p.borrow_mut();
        // p.closed = true;
        p.sink.stop();
    }
    Timer::single_shot(Duration::ZERO, || {
        PLAYER_WINDOW.with(|w| {
            w.borrow_mut().take();
        });
    });
}

impl AudioPlayer {
    fn load(
        &mut self,
        w: &MusicPlayer,
        index: usize,
    ) -> anyhow::Result<()> {
        let path = self.queue[index].clone();
        let source = Decoder::try_from(File::open(&path)?)?;

        let tagged = lofty::read_from_path(&path).ok();
        let tag = tagged
            .as_ref()
            .and_then(|t| t.primary_tag().or_else(|| t.first_tag()));

        let title = tag
            .and_then(|t| t.title().map(|s| s.to_string()))
            .or_else(|| {
                path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let artist = tag
            .and_then(|t| t.artist().map(|s| s.to_string()))
            .unwrap_or_else(|| "Unknown".into());
        let cover = tag
            .and_then(cover_of)
            .unwrap_or_else(|| self.default_art.clone());
        self.duration = tagged
            .as_ref()
            .map(|t| t.properties().duration().as_secs_f32())
            .unwrap_or(0.0);

        // a fresh player replaces (and stops) the old one
        self.sink = Player::connect_new(self._stream.mixer());
        self.sink.append(source);
        self.index = index;

        w.set_track_name(title.into());
        w.set_artist_name(artist.into());
        w.set_artwork(cover);
        w.set_duration(self.duration);
        w.set_progress(0.0);
        w.set_playing(true);
        Ok(())
    }

    fn step(&self, forward: bool, shuffle: bool) -> usize {
        let n = self.queue.len();
        if shuffle && n > 1 {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as usize)
                .unwrap_or(0);
            // 1..n-1 offset guarantees a different track
            return (self.index + 1 + nanos % (n - 1)) % n;
        }
        if forward {
            (self.index + 1) % n
        } else {
            (self.index + n - 1) % n
        }
    }
}

fn cover_of(tag: &lofty::tag::Tag) -> Option<Image> {
    let pic = tag.pictures().first()?;
    let rgba = image::load_from_memory(pic.data()).ok()?.to_rgba8();
    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    );
    Some(Image::from_rgba8(buf))
}

fn build_queue(file: &Path) -> (Vec<PathBuf>, usize) {
    let mut queue: Vec<PathBuf> = file
        .parent()
        .and_then(|d| std::fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                EXTENSIONS.contains(&e.to_lowercase().as_str())
            })
        })
        .collect();
    queue.sort();
    let index =
        queue.iter().position(|p| p == file).unwrap_or_else(|| {
            queue.push(file.to_path_buf());
            queue.len() - 1
        });
    (queue, index)
}

fn go(p: &Rc<RefCell<AudioPlayer>>, w: &MusicPlayer, forward: bool) {
    let mut p = p.borrow_mut();
    let i = p.step(forward, w.get_shuffle());
    if let Err(e) = p.load(w, i) {
        tracing::error!("failed to load track: {e}");
    }
}

pub fn music_player(
    path: impl AsRef<Path>,
) -> Result<(), slint::PlatformError> {
    let err = |e: &dyn std::fmt::Display| {
        slint::PlatformError::Other(e.to_string())
    };

    let window = MusicPlayer::new()?;
    init_theme(&window);

    let stream = DeviceSinkBuilder::open_default_sink()
        .map_err(|e| err(&e))?;
    let (queue, index) = build_queue(path.as_ref());
    let player = Rc::new(RefCell::new(AudioPlayer {
        sink: Player::connect_new(stream.mixer()),
        _stream: stream,
        queue,
        index,
        duration: 0.0,
        default_art: window.get_artwork(),
    }));
    player
        .borrow_mut()
        .load(&window, index)
        .map_err(|e| err(&e))?;

    window.on_previous({
        let (p, w) = (player.clone(), window.as_weak());
        move || {
            let Some(w) = w.upgrade() else { return };
            let restart =
                p.borrow().sink.get_pos() > Duration::from_secs(3);
            if restart {
                let _ = p.borrow().sink.try_seek(Duration::ZERO);
            } else {
                go(&p, &w, false);
            }
        }
    });

    window.on_next({
        let (p, w) = (player.clone(), window.as_weak());
        move || {
            if let Some(w) = w.upgrade() {
                go(&p, &w, true);
            }
        }
    });

    window.on_seek({
        let p = player.clone();
        move |v| {
            let p = p.borrow();
            let target =
                Duration::from_secs_f32((v * p.duration).max(0.0));
            let _ = p.sink.try_seek(target);
        }
    });

    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(50), {
        let (p, w) = (player.clone(), window.as_weak());
        move || {
            let Some(w) = w.upgrade() else { return };
            let want_playing = w.get_playing() && !w.get_scrubbing();

            let finished = {
                let p = p.borrow();
                if want_playing == p.sink.is_paused() {
                    if want_playing {
                        p.sink.play()
                    } else {
                        p.sink.pause()
                    }
                }
                if want_playing && p.duration > 0.0 {
                    let pos = p.sink.get_pos().as_secs_f32();
                    w.set_progress(
                        (pos / p.duration).clamp(0.0, 1.0),
                    );
                }
                want_playing && p.sink.empty()
            };

            if finished {
                if w.get_repeat() {
                    let i = p.borrow().index;
                    let _ = p.borrow_mut().load(&w, i);
                } else {
                    go(&p, &w, true);
                }
            }
        }
    });

    window.window().on_close_requested({
        let p = player.clone();
        move || {
            shutdown(&p);
            slint::CloseRequestResponse::HideWindow
        }
    });

    window.show()?;
    PLAYER_WINDOW.with(|w| *w.borrow_mut() = Some((window, timer)));
    Ok(())
}
