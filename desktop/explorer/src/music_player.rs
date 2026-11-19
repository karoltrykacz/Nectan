use glow::HasContext;
use image::{DynamicImage, imageops::FilterType};
use lofty::prelude::*;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};
use slint::{
    ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, Timer,
    TimerMode,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::{MusicPlayer, theme::init_theme};

const TARGET_FPS: u32 = 30;
const FRAME_INTERVAL: Duration =
    Duration::from_millis(1000 / TARGET_FPS as u64);
const RENDER_SCALE: f32 = 0.5;
const MAX_TEXTURE_DIM: u32 = 1280;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct ExtractedColor {
    pub rgb: [u8; 3],
    pub population: f32,
    pub score: f32,
}

#[derive(Clone, Copy)]
struct Oklab {
    l: f32,
    a: f32,
    b: f32,
}

struct ColorBucket {
    sum_l: f32,
    sum_a: f32,
    sum_b: f32,
    count: u32,
}

pub fn extract_smart_colors(
    img: &DynamicImage,
    max_colors: usize,
) -> Vec<ExtractedColor> {
    if max_colors == 0 {
        return Vec::new();
    }

    let small =
        img.resize_exact(64, 64, FilterType::Triangle).to_rgb8();
    let total_pixels = (64 * 64) as f32;

    let mut bins: HashMap<(u8, u8, u8), ColorBucket> = HashMap::new();

    for pixel in small.pixels() {
        let lab = srgb_to_oklab([pixel[0], pixel[1], pixel[2]]);

        let l_bin = (lab.l.clamp(0.0, 1.0) * 11.0).round() as u8;
        let a_bin = (((lab.a + 0.4) / 0.8).clamp(0.0, 1.0) * 11.0)
            .round() as u8;
        let b_bin = (((lab.b + 0.4) / 0.8).clamp(0.0, 1.0) * 11.0)
            .round() as u8;

        let entry = bins.entry((l_bin, a_bin, b_bin)).or_insert(
            ColorBucket {
                sum_l: 0.0,
                sum_a: 0.0,
                sum_b: 0.0,
                count: 0,
            },
        );

        entry.sum_l += lab.l;
        entry.sum_a += lab.a;
        entry.sum_b += lab.b;
        entry.count += 1;
    }

    struct Candidate {
        lab: Oklab,
        rgb: [u8; 3],
        population: f32,
        base_score: f32,
    }

    let mut candidates: Vec<Candidate> = bins
        .into_values()
        .filter_map(|bucket| {
            let population = bucket.count as f32 / total_pixels;
            if population < 0.002 {
                return None;
            }

            let count_f = bucket.count as f32;
            let avg_lab = Oklab {
                l: bucket.sum_l / count_f,
                a: bucket.sum_a / count_f,
                b: bucket.sum_b / count_f,
            };

            let chroma = avg_lab.a.hypot(avg_lab.b);
            let lightness_weight = 1.0
                - ((avg_lab.l - 0.55).abs() * 1.5).clamp(0.0, 0.8);
            let base_score = population.powf(0.35)
                * (chroma + 0.015).powf(1.7)
                * lightness_weight;

            Some(Candidate {
                lab: avg_lab,
                rgb: oklab_to_srgb(avg_lab),
                population,
                base_score,
            })
        })
        .collect();

    if candidates.is_empty() {
        return Vec::new();
    }

    let mut selected: Vec<(Oklab, ExtractedColor)> =
        Vec::with_capacity(max_colors);

    candidates.sort_by(|a, b| b.population.total_cmp(&a.population));
    let dominant = candidates.remove(0);
    selected.push((
        dominant.lab,
        ExtractedColor {
            rgb: dominant.rgb,
            population: dominant.population,
            score: dominant.base_score,
        },
    ));

    let min_oklab_distance = 0.09;

    while selected.len() < max_colors && !candidates.is_empty() {
        let mut best_idx = None;
        let mut best_adjusted_score = -1.0;

        for (i, cand) in candidates.iter().enumerate() {
            let min_dist = selected
                .iter()
                .map(|(sel_lab, _)| {
                    oklab_distance(cand.lab, *sel_lab)
                })
                .fold(f32::INFINITY, f32::min);

            if min_dist < min_oklab_distance {
                continue;
            }

            let diversity_multiplier = min_dist.clamp(0.09, 0.35);
            let adjusted_score =
                cand.base_score * diversity_multiplier;

            if adjusted_score > best_adjusted_score {
                best_adjusted_score = adjusted_score;
                best_idx = Some(i);
            }
        }

        match best_idx {
            Some(idx) => {
                let chosen = candidates.swap_remove(idx);
                selected.push((
                    chosen.lab,
                    ExtractedColor {
                        rgb: chosen.rgb,
                        population: chosen.population,
                        score: best_adjusted_score,
                    },
                ));
            }
            None => break,
        }
    }

    selected.into_iter().map(|(_, color)| color).collect()
}

fn oklab_distance(x: Oklab, y: Oklab) -> f32 {
    ((x.l - y.l).powi(2) + (x.a - y.a).powi(2) + (x.b - y.b).powi(2))
        .sqrt()
}

fn srgb_to_linear(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

fn srgb_to_oklab(rgb: [u8; 3]) -> Oklab {
    let r = srgb_to_linear(rgb[0]);
    let g = srgb_to_linear(rgb[1]);
    let b = srgb_to_linear(rgb[2]);

    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
        .cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
        .cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
        .cbrt();

    Oklab {
        l: 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        a: 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        b: 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    }
}

fn oklab_to_srgb(lab: Oklab) -> [u8; 3] {
    let l_ = lab.l + 0.3963377774 * lab.a + 0.2158037573 * lab.b;
    let m_ = lab.l - 0.1055613458 * lab.a - 0.0638541728 * lab.b;
    let s_ = lab.l - 0.0894841775 * lab.a - 1.2914855480 * lab.b;

    let l = l_.powi(3);
    let m = m_.powi(3);
    let s = s_.powi(3);

    [
        linear_to_srgb(
            4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        ),
        linear_to_srgb(
            -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        ),
        linear_to_srgb(
            -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
        ),
    ]
}

const STOPS: usize = 5;
type Rgb = [u8; 3];

const MONO: [Rgb; STOPS] = [
    [0x05, 0x05, 0x05],
    [0x2a, 0x2a, 0x2a],
    [0x6b, 0x6b, 0x6b],
    [0xb8, 0xb8, 0xb8],
    [0xf5, 0xf5, 0xf5],
];

fn luma(c: &Rgb) -> f32 {
    0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32
}

fn gradient_palette(img: Option<&DynamicImage>) -> [Rgb; STOPS] {
    let Some(img) = img else {
        return MONO;
    };

    let mut rgb: Vec<Rgb> = extract_smart_colors(img, STOPS)
        .iter()
        .map(|c| c.rgb)
        .collect();
    if rgb.is_empty() {
        return MONO;
    }

    if rgb.len() == 1 {
        let c = rgb[0];
        rgb.push(c.map(|v| (v as f32 * 0.35) as u8));
        rgb.push(
            c.map(|v| (v as f32 + (255.0 - v as f32) * 0.6) as u8),
        );
    }

    rgb.sort_by(|a, b| luma(a).total_cmp(&luma(b)));

    std::array::from_fn(|i| {
        let t =
            i as f32 / (STOPS - 1) as f32 * (rgb.len() - 1) as f32;
        let lo = t.floor() as usize;
        let hi = (lo + 1).min(rgb.len() - 1);
        let f = t - lo as f32;
        let ch = |k: usize| {
            (rgb[lo][k] as f32 * (1.0 - f) + rgb[hi][k] as f32 * f)
                .round() as u8
        };
        [ch(0), ch(1), ch(2)]
    })
}

fn to_oklab_vec(rgb: &[Rgb]) -> Vec<[f32; 3]> {
    rgb.iter()
        .map(|&c| {
            let l = srgb_to_oklab(c);
            [l.l, l.a, l.b]
        })
        .collect()
}

const MAX_COLORS: usize = 8;
const VERT_SRC: &str = r#"#version 100
attribute vec2 position;
varying vec2 frag_position;
void main() {
    frag_position = position;
    gl_Position = vec4(position, 0.0, 1.0);
}"#;
const FRAG_SRC: &str =
    include_str!("../../shared/shaders/mesh_gradient.frag");

const LOOK_SCALE: f32 = 1.0;
const LOOK_CURL: f32 = 0.55;
const LOOK_WARP: f32 = 0.5;
const LOOK_OPENNESS: f32 = 0.28;
const LOOK_GRAIN: f32 = 0.05;
const LOOK_SPEED: f32 = 0.1;
const LOOK_SEED: f32 = 7.0;

struct Params {
    colors: Vec<[f32; 3]>,
    scale: f32,
    curl: f32,
    warp: f32,
    openness: f32,
    grain: f32,
    speed: f32,
    seed: f32,
    width: u32,
    height: u32,
}

struct Target {
    texture: glow::Texture,
    fbo: glow::Framebuffer,
    width: u32,
    height: u32,
}

impl Target {
    fn new(gl: &glow::Context, width: u32, height: u32) -> Self {
        unsafe {
            let prev_tex =
                gl.get_parameter_texture(glow::TEXTURE_BINDING_2D);
            let prev_fbo = gl
                .get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);

            let texture =
                gl.create_texture().expect("create texture");
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            for (pname, value) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    pname,
                    value as i32,
                );
            }
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as _,
                width as _,
                height as _,
                0,
                glow::RGBA as _,
                glow::UNSIGNED_BYTE as _,
                glow::PixelUnpackData::Slice(None),
            );

            let fbo =
                gl.create_framebuffer().expect("create framebuffer");
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            debug_assert_eq!(
                gl.check_framebuffer_status(glow::FRAMEBUFFER),
                glow::FRAMEBUFFER_COMPLETE
            );

            gl.bind_framebuffer(glow::FRAMEBUFFER, prev_fbo);
            gl.bind_texture(glow::TEXTURE_2D, prev_tex);
            Self {
                texture,
                fbo,
                width,
                height,
            }
        }
    }

    fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_framebuffer(self.fbo);
            gl.delete_texture(self.texture);
        }
    }
}

const TOGGLED_CAPS: [u32; 6] = [
    glow::BLEND,
    glow::CULL_FACE,
    glow::DEPTH_TEST,
    glow::STENCIL_TEST,
    glow::SCISSOR_TEST,
    glow::DITHER,
];

struct Locations {
    resolution: Option<glow::UniformLocation>,
    time: Option<glow::UniformLocation>,
    seed: Option<glow::UniformLocation>,
    scale: Option<glow::UniformLocation>,
    curl: Option<glow::UniformLocation>,
    warp: Option<glow::UniformLocation>,
    openness: Option<glow::UniformLocation>,
    grain: Option<glow::UniformLocation>,
    count: Option<glow::UniformLocation>,
    colors: [Option<glow::UniformLocation>; MAX_COLORS],
}

impl Locations {
    fn new(gl: &glow::Context, program: glow::Program) -> Self {
        let l =
            |n: &str| unsafe { gl.get_uniform_location(program, n) };
        Self {
            resolution: l("u_resolution"),
            time: l("u_time"),
            seed: l("u_seed"),
            scale: l("u_scale"),
            curl: l("u_curl"),
            warp: l("u_warp"),
            openness: l("u_openness"),
            grain: l("u_grain"),
            count: l("u_count"),
            colors: std::array::from_fn(|i| l(&format!("u_c{i}"))),
        }
    }
}

pub struct MeshRenderer {
    gl: glow::Context,
    program: glow::Program,
    vbo: glow::Buffer,
    vao: glow::VertexArray,
    displayed: Target,
    next: Target,
    time: f32,
    last_frame: Instant,
    locs: Locations,
    last_colors: Vec<[f32; 3]>,
    last_size: (u32, u32),
}

impl MeshRenderer {
    fn new(gl: glow::Context) -> Self {
        unsafe {
            let program =
                gl.create_program().expect("create program");
            for (kind, src) in [
                (glow::VERTEX_SHADER, VERT_SRC),
                (glow::FRAGMENT_SHADER, FRAG_SRC),
            ] {
                let sh =
                    gl.create_shader(kind).expect("create shader");
                gl.shader_source(sh, src);
                gl.compile_shader(sh);
                if !gl.get_shader_compile_status(sh) {
                    panic!(
                        "shader compile error: {}",
                        gl.get_shader_info_log(sh)
                    );
                }
                gl.attach_shader(program, sh);
            }
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                panic!(
                    "program link error: {}",
                    gl.get_program_info_log(program)
                );
            }

            let pos_loc = gl
                .get_attrib_location(program, "position")
                .expect("position attrib");

            let locs = Locations::new(&gl, program);

            let prev_vbo =
                gl.get_parameter_buffer(glow::ARRAY_BUFFER_BINDING);
            let prev_vao = gl.get_parameter_vertex_array(
                glow::VERTEX_ARRAY_BINDING,
            );

            let vertices: [f32; 8] =
                [-1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0, -1.0];
            let bytes = std::slice::from_raw_parts(
                vertices.as_ptr() as *const u8,
                std::mem::size_of_val(&vertices),
            );
            let vbo = gl.create_buffer().expect("create buffer");
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytes,
                glow::STATIC_DRAW,
            );

            let vao = gl.create_vertex_array().expect("create vao");
            gl.bind_vertex_array(Some(vao));
            gl.enable_vertex_attrib_array(pos_loc);
            gl.vertex_attrib_pointer_f32(
                pos_loc,
                2,
                glow::FLOAT,
                false,
                8,
                0,
            );

            gl.bind_buffer(glow::ARRAY_BUFFER, prev_vbo);
            gl.bind_vertex_array(prev_vao);

            let displayed = Target::new(&gl, 320, 200);
            let next = Target::new(&gl, 320, 200);

            Self {
                gl,
                program,
                vbo,
                vao,
                displayed,
                next,
                time: 0.0,
                last_frame: Instant::now(),
                locs,
                last_colors: Vec::new(),
                last_size: (0, 0),
            }
        }
    }

    fn render(&mut self, p: &Params) -> slint::Image {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_frame)
            .as_secs_f32()
            .min(0.1);
        self.last_frame = now;
        self.time += dt * p.speed;

        let (w, h) = (p.width.max(1), p.height.max(1));

        unsafe {
            let gl = &self.gl;

            if self.next.width != w || self.next.height != h {
                let new = Target::new(gl, w, h);
                let old = std::mem::replace(&mut self.next, new);
                old.destroy(gl);
            }

            let prev_fbo = gl
                .get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);
            let prev_vbo =
                gl.get_parameter_buffer(glow::ARRAY_BUFFER_BINDING);
            let prev_vao = gl.get_parameter_vertex_array(
                glow::VERTEX_ARRAY_BINDING,
            );
            let mut vp = [0i32; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut vp);
            let prev_caps = TOGGLED_CAPS.map(|c| gl.is_enabled(c));

            for c in TOGGLED_CAPS {
                gl.disable(c);
            }
            gl.color_mask(true, true, true, true);

            gl.bind_framebuffer(
                glow::FRAMEBUFFER,
                Some(self.next.fbo),
            );
            gl.viewport(0, 0, w as i32, h as i32);
            gl.use_program(Some(self.program));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            gl.bind_vertex_array(Some(self.vao));

            let l = &self.locs;
            if self.last_size != (w, h) {
                gl.uniform_2_f32(
                    l.resolution.as_ref(),
                    w as f32,
                    h as f32,
                );
                self.last_size = (w, h);
            }
            gl.uniform_1_f32(l.time.as_ref(), self.time);

            if self.last_colors != p.colors {
                gl.uniform_1_f32(l.seed.as_ref(), p.seed);
                gl.uniform_1_f32(l.scale.as_ref(), p.scale);
                gl.uniform_1_f32(l.curl.as_ref(), p.curl);
                gl.uniform_1_f32(l.warp.as_ref(), p.warp);
                gl.uniform_1_f32(l.openness.as_ref(), p.openness);
                gl.uniform_1_f32(l.grain.as_ref(), p.grain);

                let n = p.colors.len().clamp(1, MAX_COLORS);
                gl.uniform_1_f32(l.count.as_ref(), n.max(2) as f32);
                for i in 0..MAX_COLORS {
                    let c = p
                        .colors
                        .get(i.min(n - 1))
                        .copied()
                        .unwrap_or([0.0; 3]);
                    gl.uniform_3_f32(
                        l.colors[i].as_ref(),
                        c[0],
                        c[1],
                        c[2],
                    );
                }
                self.last_colors = p.colors.clone();
            }

            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);

            gl.use_program(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, prev_vbo);
            gl.bind_vertex_array(prev_vao);
            gl.bind_framebuffer(glow::FRAMEBUFFER, prev_fbo);
            gl.viewport(vp[0], vp[1], vp[2], vp[3]);
            for (cap, was_on) in TOGGLED_CAPS.iter().zip(prev_caps) {
                if was_on {
                    gl.enable(*cap);
                }
            }
        }

        let size = (self.next.width, self.next.height).into();
        let image = unsafe {
            slint::BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(
                self.next.texture.0,
                size,
            )
            .build()
        };
        std::mem::swap(&mut self.next, &mut self.displayed);
        image
    }
}

impl Drop for MeshRenderer {
    fn drop(&mut self) {
        self.displayed.destroy(&self.gl);
        self.next.destroy(&self.gl);
        unsafe {
            self.gl.delete_program(self.program);
            self.gl.delete_vertex_array(self.vao);
            self.gl.delete_buffer(self.vbo);
        }
    }
}

const EXTENSIONS: &[&str] = &["mp3", "flac", "ogg", "wav"];

type Palette = Rc<RefCell<Vec<[f32; 3]>>>;

thread_local! {
    static PLAYER_WINDOW: RefCell<Option<(MusicPlayer, Timer, Timer)>> = RefCell::new(None);
}

struct AudioPlayer {
    _stream: MixerDeviceSink,
    sink: Player,
    queue: Vec<PathBuf>,
    index: usize,
    duration: f32,
    default_art: Image,
    palette: Palette,
}

fn shutdown(p: &Rc<RefCell<AudioPlayer>>) {
    {
        let p = p.borrow_mut();
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

        let cover_data = tag.and_then(cover_of);
        let colors =
            gradient_palette(cover_data.as_ref().map(|(_, img)| img));
        *self.palette.borrow_mut() = to_oklab_vec(&colors);

        let cover = match cover_data {
            Some((image, _)) => image,
            None => self.default_art.clone(),
        };

        self.duration = tagged
            .as_ref()
            .map(|t| t.properties().duration().as_secs_f32())
            .unwrap_or(0.0);

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
            return (self.index + 1 + nanos % (n - 1)) % n;
        }
        if forward {
            (self.index + 1) % n
        } else {
            (self.index + n - 1) % n
        }
    }
}

fn cover_of(tag: &lofty::tag::Tag) -> Option<(Image, DynamicImage)> {
    let pic = tag.pictures().first()?;
    let rgba = image::load_from_memory(pic.data()).ok()?.to_rgba8();
    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    );
    Some((Image::from_rgba8(buf), DynamicImage::ImageRgba8(rgba)))
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

fn quantize_dim(v: i32) -> u32 {
    let v = ((v.max(1) as f32 * RENDER_SCALE).ceil() as u32).max(16);
    (v.div_ceil(16) * 16).min(MAX_TEXTURE_DIM)
}

pub fn music_player(
    path: impl AsRef<Path>,
) -> Result<(), slint::PlatformError> {
    let err = |e: &dyn std::fmt::Display| {
        slint::PlatformError::Other(e.to_string())
    };

    let window = MusicPlayer::new()?;
    init_theme(&window);

    let palette: Palette = Rc::new(RefCell::new(to_oklab_vec(&MONO)));

    let mut renderer: Option<MeshRenderer> = None;
    let mut pending_gl: Option<glow::Context> = None;
    let weak = window.as_weak();
    let gl_palette = palette.clone();
    let start = Instant::now();
    let mut last_render =
        start.checked_sub(FRAME_INTERVAL).unwrap_or(start);
    window
        .window()
        .set_rendering_notifier(
            move |state, graphics_api| match state {
                slint::RenderingState::RenderingSetup => {
                    let gl = match graphics_api {
                        slint::GraphicsAPI::NativeOpenGL {
                            get_proc_address,
                        } => unsafe {
                            glow::Context::from_loader_function_cstr(
                                |s| get_proc_address(s),
                            )
                        },
                        _ => return,
                    };
                    renderer = None;
                    pending_gl = Some(gl);
                }
                slint::RenderingState::BeforeRendering => {
                    if renderer.is_none() {
                        if let Some(gl) = pending_gl.take() {
                            renderer = Some(MeshRenderer::new(gl));
                            // force a render on the first frame
                            last_render = Instant::now()
                                .checked_sub(FRAME_INTERVAL)
                                .unwrap_or_else(Instant::now);
                        }
                    }

                    // Frame limiter: other redraws (hover, progress bar,
                    // ...) just reuse the last gradient texture.
                    if last_render.elapsed()
                        < FRAME_INTERVAL.mul_f32(0.8)
                    {
                        return;
                    }

                    if let (Some(r), Some(app)) =
                        (renderer.as_mut(), weak.upgrade())
                    {
                        if !app.window().is_visible() {
                            return;
                        }

                        let colors = gl_palette
                            .borrow()
                            .iter()
                            .copied()
                            .take(MAX_COLORS)
                            .collect();
                        let image = r.render(&Params {
                            colors,
                            scale: LOOK_SCALE,
                            curl: LOOK_CURL,
                            warp: LOOK_WARP,
                            openness: LOOK_OPENNESS,
                            grain: LOOK_GRAIN,
                            speed: LOOK_SPEED,
                            seed: LOOK_SEED,
                            width: quantize_dim(
                                app.get_requested_texture_width(),
                            ),
                            height: quantize_dim(
                                app.get_requested_texture_height(),
                            ),
                        });
                        app.set_texture(image);
                        last_render = Instant::now();
                    }
                }
                slint::RenderingState::RenderingTeardown => {
                    drop(renderer.take())
                }
                _ => {}
            },
        )
        .expect("Unable to set rendering notifier");

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
        palette,
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

    let redraw_timer = Timer::default();
    redraw_timer.start(TimerMode::Repeated, FRAME_INTERVAL, {
        let w = window.as_weak();
        move || {
            if let Some(w) = w.upgrade() {
                w.window().request_redraw();
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
    PLAYER_WINDOW.with(|w| {
        *w.borrow_mut() = Some((window, timer, redraw_timer))
    });
    Ok(())
}
