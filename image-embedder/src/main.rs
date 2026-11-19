use anyhow::Result;
use image::DynamicImage;
use indicatif::{ProgressBar, ProgressStyle};
use iroh::{
    Endpoint,
    endpoint::{Connection, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use ndarray::{Array2, Array4};
use ort::{
    session::{Session, builder::GraphOptimizationLevel},
    value::TensorRef,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    sync::{Arc, Mutex},
};
use tokenizers::Tokenizer;

const ALPN: &[u8] = b"nectan/photo-embedder/0";
const MAX_PAYLOAD_SIZE: usize = 20 * 1024 * 1024;

// SigLIP 2 base (224x224 -> 768-dim L2-normalized embeddings)
const EXPECTED_WIDTH: u32 = 224;
const EXPECTED_HEIGHT: u32 = 224;
const MEAN: [f32; 3] = [0.5, 0.5, 0.5];
const STD: [f32; 3] = [0.5, 0.5, 0.5];
const MAX_TEXT_TOKENS: usize = 64;

const BASE_URL: &str = "https://huggingface.co/onnx-community/siglip2-base-patch16-224-ONNX/resolve/main";
const MODEL_DIR: &str = "models/siglip2-base-224";

#[derive(Debug, Serialize, Deserialize)]
pub enum EmbedRequest {
    Image(Vec<u8>),
    Text(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EmbedResponse {
    Ok(Vec<f32>),
    Err(String),
}

struct Siglip2Pipeline {
    vision_model: Session,
    text_model: Session,
    tokenizer: Tokenizer,
}

impl std::fmt::Debug for Siglip2Pipeline {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.debug_struct("Siglip2Pipeline").finish()
    }
}

#[derive(Debug, Clone)]
struct PhotoEmbedderServer {
    pipeline: Arc<Mutex<Siglip2Pipeline>>,
}

fn l2_normalize(mut vec: Vec<f32>) -> Vec<f32> {
    let norm = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-12 {
        for x in &mut vec {
            *x /= norm;
        }
    }
    vec
}

impl PhotoEmbedderServer {
    fn embed_image(
        pipe: &mut Siglip2Pipeline,
        img: &DynamicImage,
    ) -> Result<Vec<f32>> {
        if img.width() != EXPECTED_WIDTH
            || img.height() != EXPECTED_HEIGHT
        {
            anyhow::bail!(
                "expected {}x{} image, got {}x{}",
                EXPECTED_WIDTH,
                EXPECTED_HEIGHT,
                img.width(),
                img.height()
            );
        }

        let rgb = img.to_rgb8();
        let mut pixel_values = Array4::<f32>::zeros((
            1,
            3,
            EXPECTED_HEIGHT as usize,
            EXPECTED_WIDTH as usize,
        ));

        for (x, y, pixel) in rgb.enumerate_pixels() {
            let [r, g, b] = pixel.0;
            pixel_values[[0, 0, y as usize, x as usize]] =
                ((r as f32 / 255.0) - MEAN[0]) / STD[0];
            pixel_values[[0, 1, y as usize, x as usize]] =
                ((g as f32 / 255.0) - MEAN[1]) / STD[1];
            pixel_values[[0, 2, y as usize, x as usize]] =
                ((b as f32 / 255.0) - MEAN[2]) / STD[2];
        }

        let outputs = pipe
            .vision_model
            .run(ort::inputs!["pixel_values" => TensorRef::from_array_view(&pixel_values)?])?;

        let (_shape, data) =
            outputs["pooler_output"].try_extract_tensor::<f32>()?;
        Ok(l2_normalize(data.to_vec()))
    }

    fn embed_text(
        pipe: &mut Siglip2Pipeline,
        query: &str,
    ) -> Result<Vec<f32>> {
        let normalized_query = query.trim().to_lowercase();
        let encoding = pipe
            .tokenizer
            .encode(normalized_query, true)
            .map_err(|e| anyhow::anyhow!("tokenizer error: {e}"))?;

        let mut ids: Vec<i64> = encoding
            .get_ids()
            .iter()
            .take(MAX_TEXT_TOKENS)
            .map(|&id| id as i64)
            .collect();
        ids.resize(MAX_TEXT_TOKENS, 1); // pad_token_id = 1

        let input_ids =
            Array2::from_shape_vec((1, MAX_TEXT_TOKENS), ids)?;
        let outputs = pipe
            .text_model
            .run(ort::inputs!["input_ids" => TensorRef::from_array_view(&input_ids)?])?;

        let (_shape, data) =
            outputs["pooler_output"].try_extract_tensor::<f32>()?;
        Ok(l2_normalize(data.to_vec()))
    }
}

impl ProtocolHandler for PhotoEmbedderServer {
    async fn accept(
        &self,
        connection: Connection,
    ) -> Result<(), AcceptError> {
        loop {
            let (mut send, mut recv) = connection.accept_bi().await?;
            let pipeline = self.pipeline.clone();

            tokio::spawn(async move {
                let response = match recv
                    .read_to_end(MAX_PAYLOAD_SIZE)
                    .await
                {
                    Ok(bytes) => match postcard::from_bytes::<
                        EmbedRequest,
                    >(&bytes)
                    {
                        Ok(req) => {
                            let res = tokio::task::spawn_blocking(move || -> Result<Vec<f32>> {
                                let mut pipe = pipeline
                                    .lock()
                                    .map_err(|_| anyhow::anyhow!("pipeline mutex poisoned"))?;
                                match req {
                                    EmbedRequest::Image(img_bytes) => {
                                        // Decoded in RAM and dropped immediately; nothing persisted to disk
                                        let img = image::load_from_memory(&img_bytes)?;
                                        Self::embed_image(&mut pipe, &img)
                                    }
                                    EmbedRequest::Text(query) => Self::embed_text(&mut pipe, &query),
                                }
                            })
                            .await;

                            match res {
                                Ok(Ok(vec)) => EmbedResponse::Ok(vec),
                                Ok(Err(e)) => {
                                    EmbedResponse::Err(e.to_string())
                                }
                                Err(e) => EmbedResponse::Err(
                                    format!("task join error: {e}"),
                                ),
                            }
                        }
                        Err(e) => EmbedResponse::Err(format!(
                            "invalid request: {e}"
                        )),
                    },
                    Err(e) => EmbedResponse::Err(format!(
                        "read stream error: {e}"
                    )),
                };

                if let Ok(encoded) = postcard::to_allocvec(&response)
                {
                    let _ = send.write_all(&encoded).await;
                    let _ = send.finish();
                }
            });
        }
    }
}

fn download_file_if_missing(path: &Path, url: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }

    println!("Downloading {}...", path.display());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension("tmp");
    let mut response = ureq::get(url).call()?;
    let total_size = response.body().content_length();

    let pb = match total_size {
        Some(len) => {
            let bar = ProgressBar::new(len);
            bar.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
                )?
                .progress_chars("#>-"),
            );
            bar
        }
        None => ProgressBar::new_spinner(),
    };

    let mut reader = response.body_mut().as_reader();
    let mut file = File::create(&tmp_path)?;
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;
        pb.set_position(downloaded);
    }

    file.sync_all()?;
    drop(file);
    pb.finish_and_clear();
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn build_session(
    env: &ort::environment::Environment,
    path: &Path,
) -> Result<Session> {
    Session::builder(env)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .with_intra_threads(4)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .commit_from_file(path)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn load_siglip2(dir: impl AsRef<Path>) -> Result<Siglip2Pipeline> {
    let dir = dir.as_ref();

    let files = [
        (
            "vision_model.onnx",
            format!("{BASE_URL}/onnx/vision_model_fp16.onnx"),
        ),
        (
            "text_model.onnx",
            format!("{BASE_URL}/onnx/text_model_fp16.onnx"),
        ),
        ("tokenizer.json", format!("{BASE_URL}/tokenizer.json")),
    ];

    for (local_name, url) in &files {
        download_file_if_missing(&dir.join(local_name), url)?;
    }

    println!("Loading SigLIP 2 ONNX sessions...");
    let env = ort::init().with_name("nectan-siglip2").build()?;

    let vision_model =
        build_session(&env, &dir.join("vision_model.onnx"))?;
    let text_model =
        build_session(&env, &dir.join("text_model.onnx"))?;
    let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
        .map_err(|e| {
            anyhow::anyhow!("failed to load tokenizer: {e}")
        })?;

    Ok(Siglip2Pipeline {
        vision_model,
        text_model,
        tokenizer,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let pipeline = load_siglip2(MODEL_DIR)?;

    let server = PhotoEmbedderServer {
        pipeline: Arc::new(Mutex::new(pipeline)),
    };

    let endpoint = Endpoint::bind(presets::N0).await?;
    endpoint.online().await;
    println!("Node ID: {}", endpoint.id());

    let router =
        Router::builder(endpoint).accept(ALPN, server).spawn();

    tokio::signal::ctrl_c().await?;
    router.shutdown().await?;

    Ok(())
}
