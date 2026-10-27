use anyhow::{Result, bail};
use ed25519_dalek::Signature;
use iroh::EndpointId;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{error, info, warn};

use crate::{
    devices::DeviceId,
    protocol::{DISCOVERY_URL, NectanState},
};

pub const CODE_SESSION_TTL: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeLookupError {
    NotFound,
    ConnectionFailed,
    NectanService,
}

impl std::fmt::Display for CodeLookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodeLookupError::NotFound => write!(f, "Code not found."),
            CodeLookupError::ConnectionFailed => {
                write!(f, "Connection failed.")
            }
            CodeLookupError::NectanService => {
                write!(f, "Nectan service error.")
            }
        }
    }
}

impl std::error::Error for CodeLookupError {}

#[derive(Deserialize, Serialize)]
pub struct CodeLookupRequest {
    pub code: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CodeLookupResponse {
    pub device_id: EndpointId,
}

pub async fn lookup_code(
    code: String,
    state: &NectanState,
) -> Result<EndpointId, CodeLookupError> {
    let client = state.http();
    let payload = CodeLookupRequest { code };
    let payload = postcard::to_allocvec(&payload).unwrap();

    match client
        .post(format!("{DISCOVERY_URL}/find_code_session"))
        .body(payload)
        .send()
        .await
    {
        Ok(response) => match response.status() {
            StatusCode::OK => {
                let body = response
                    .bytes()
                    .await
                    .map_err(|_| CodeLookupError::ConnectionFailed)?;

                let body = postcard::from_bytes::<CodeLookupResponse>(&body)
                    .map_err(|e| {
                        error!("Failed to lookup code. {e:?}");
                        println!("Failed to Deserialize code response.");
                        CodeLookupError::NectanService
                    })?;

                Ok(body.device_id)
            }
            StatusCode::REQUEST_TIMEOUT => {
                info!("Code was found but expired.");
                Err(CodeLookupError::NotFound)
            }
            StatusCode::NOT_FOUND => {
                info!("Code not found");

                Err(CodeLookupError::NotFound)
            }
            _ => Err(CodeLookupError::NectanService),
        },
        Err(_) => Err(CodeLookupError::ConnectionFailed),
    }
}

#[derive(Serialize, Deserialize)]
pub struct CodeIssueRequest {
    pub device_id: DeviceId,
    pub code: String,
    pub seq_num: u64,
    pub signature: Signature,
}

pub async fn issue_code(state: &NectanState, code: String) -> Result<String> {
    let Some(seq_n) = state.seq_num.get_and_inc() else {
        error!("Endpoint not announced.");
        bail!(
            "Connection error. Make sure you are connected to the internet and try again."
        )
    };

    let signature = state.sign(&seq_n.to_be_bytes());

    let payload = CodeIssueRequest {
        device_id: state.device_id(),
        code: code.clone(),
        seq_num: seq_n,
        signature,
    };
    let payload = postcard::to_allocvec(&payload).unwrap();

    let url = format!("{DISCOVERY_URL}/issue_code");
    let max_attempts = 3;
    let mut delay_ms = 200;

    for attempt in 1..=max_attempts {
        info!("Attempting to issue code");

        match state.http().post(&url).body(payload.clone()).send().await {
            Ok(response) => match response.status() {
                StatusCode::ACCEPTED => {
                    info!("Code issued {:?}", code);
                    return Ok(code);
                }
                StatusCode::CREATED => {
                    let code = response
                        .text()
                        .await
                        .unwrap_or("Bad response".to_string());
                    warn!("(CREATED) Code issued {:?}", code);
                    return Ok(code);
                }
                s => {
                    error!("Failed to issue code. Status code {}", s);
                }
            },
            Err(e) => {
                error!(
                    "Failed to issue code. Retrying {}/{}. {}",
                    attempt, max_attempts, e
                );
            }
        }

        if attempt < max_attempts {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            delay_ms *= 2;
        }
    }
    bail!(
        "Connection error. Make sure you are connected to the internet and try again."
    )
}

pub fn gen_code() -> String {
    let mut r = String::new();
    for _ in 0..6 {
        let rand = rand::random::<u64>() % 10;
        r += &rand.to_string();
    }
    r
}
