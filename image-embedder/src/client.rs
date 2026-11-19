use anyhow::{Context, Result};
use image::{ImageFormat, imageops::FilterType};
use iroh::{
    Endpoint, EndpointAddr, EndpointId,
    endpoint::{Connection, presets},
};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, BufRead, Cursor, Write},
    path::Path,
};

pub const ALPN: &[u8] = b"nectan/photo-embedder/0";
const MAX_RESPONSE_SIZE: usize = 64 * 1024;
const TARGET_SIZE: u32 = 224;

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

pub struct PhotoEmbedderClient {
    endpoint: Endpoint,
    conn: Connection,
}

impl PhotoEmbedderClient {
    pub async fn connect(
        addr: impl Into<EndpointAddr>,
    ) -> Result<Self> {
        let endpoint = Endpoint::bind(presets::N0).await?;
        let conn = endpoint.connect(addr, ALPN).await?;
        Ok(Self { endpoint, conn })
    }

    async fn send(&self, req: &EmbedRequest) -> Result<Vec<f32>> {
        let payload = postcard::to_allocvec(req)
            .context("failed to serialize EmbedRequest")?;

        let (mut send, mut recv) = self.conn.open_bi().await?;
        send.write_all(&payload).await?;
        send.finish()?;

        let response_bytes =
            recv.read_to_end(MAX_RESPONSE_SIZE).await?;
        let response: EmbedResponse =
            postcard::from_bytes(&response_bytes)
                .context("failed to deserialize EmbedResponse")?;

        match response {
            EmbedResponse::Ok(embedding) => Ok(embedding),
            EmbedResponse::Err(err) => {
                anyhow::bail!("server error: {err}")
            }
        }
    }

    pub async fn embed_image_file(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<Vec<f32>> {
        let path = path.as_ref().to_path_buf();

        let resized_bytes = tokio::task::spawn_blocking(
            move || -> Result<Vec<u8>> {
                let img = image::open(&path).with_context(|| {
                    format!("failed to open image {}", path.display())
                })?;
                let resized = img.resize_exact(
                    TARGET_SIZE,
                    TARGET_SIZE,
                    FilterType::Triangle,
                );

                let mut buf = Vec::new();
                resized.write_to(
                    &mut Cursor::new(&mut buf),
                    ImageFormat::Png,
                )?;
                Ok(buf)
            },
        )
        .await??;

        self.send(&EmbedRequest::Image(resized_bytes)).await
    }

    /// Sends a text string and returns its 768-dim embedding in the same vector space
    pub async fn embed_text(
        &self,
        text: impl Into<String>,
    ) -> Result<Vec<f32>> {
        self.send(&EmbedRequest::Text(text.into())).await
    }

    pub async fn close(self) {
        self.conn.close(0u8.into(), b"done");
        self.endpoint.close().await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    print!("Enter server id: ");
    io::stdout().flush()?;

    let id_str =
        lines.next().context("EOF before reading server id")??;
    let server_id: EndpointId =
        id_str.trim().parse().context("invalid EndpointId")?;

    println!("Connecting to {server_id}...");
    let client = PhotoEmbedderClient::connect(server_id).await?;
    println!(
        "Connected! Enter image path (or 'text: <query>' or 'exit'):"
    );

    loop {
        print!("> ");
        io::stdout().flush()?;

        let Some(line) = lines.next() else {
            break;
        };

        let line = line?;
        let input = line.trim().trim_matches('"');
        if input.is_empty() {
            continue;
        }
        if input.eq_ignore_ascii_case("exit")
            || input.eq_ignore_ascii_case("quit")
        {
            break;
        }

        if let Some(query) = input.strip_prefix("text:") {
            match client.embed_text(query.trim()).await {
                Ok(vec) => println!(
                    "Text embedding ({} dims): {:?}...",
                    vec.len(),
                    &vec[..5]
                ),
                Err(e) => eprintln!("Error: {e:#}"),
            }
        } else {
            match client.embed_image_file(input).await {
                Ok(vec) => println!(
                    "Image embedding ({} dims): {:?}...",
                    vec.len(),
                    &vec[..5]
                ),
                Err(e) => eprintln!("Error: {e:#}"),
            }
        }
    }

    client.close().await;
    Ok(())
}
