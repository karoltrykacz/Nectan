use data_encoding::{
    BASE32, BASE32_NOPAD, BASE32HEX, BASE64, BASE64_NOPAD, BASE64URL,
    BASE64URL_NOPAD, HEXLOWER, HEXUPPER,
};
pub use hashing::{Algorithm, HashError};
use std::{path::Path, str::FromStr};

type Result<T> = std::result::Result<T, HashError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Hex,
    HexUpper,
    Base64,
    Base64NoPad,
    Base64Url,
    Base64UrlNoPad,
    Base32,
    Base32NoPad,
    Base32Hex,
    Binary,
    Decimal,
}

impl OutputFormat {
    pub fn all() -> [OutputFormat; 11] {
        [
            OutputFormat::Hex,
            OutputFormat::HexUpper,
            OutputFormat::Base64,
            OutputFormat::Base64NoPad,
            OutputFormat::Base64Url,
            OutputFormat::Base64UrlNoPad,
            OutputFormat::Base32,
            OutputFormat::Base32NoPad,
            OutputFormat::Base32Hex,
            OutputFormat::Binary,
            OutputFormat::Decimal,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            OutputFormat::Hex => "hex",
            OutputFormat::HexUpper => "hex-upper",
            OutputFormat::Base64 => "base64",
            OutputFormat::Base64NoPad => "base64-nopad",
            OutputFormat::Base64Url => "base64-url",
            OutputFormat::Base64UrlNoPad => "base64-url-nopad",
            OutputFormat::Base32 => "base32",
            OutputFormat::Base32NoPad => "base32-nopad",
            OutputFormat::Base32Hex => "base32-hex",
            OutputFormat::Binary => "binary",
            OutputFormat::Decimal => "decimal",
        }
    }

    pub fn encode(&self, bytes: &[u8]) -> String {
        match self {
            OutputFormat::Hex => HEXLOWER.encode(bytes),
            OutputFormat::HexUpper => HEXUPPER.encode(bytes),
            OutputFormat::Base64 => BASE64.encode(bytes),
            OutputFormat::Base64NoPad => BASE64_NOPAD.encode(bytes),
            OutputFormat::Base64Url => BASE64URL.encode(bytes),
            OutputFormat::Base64UrlNoPad => {
                BASE64URL_NOPAD.encode(bytes)
            }
            OutputFormat::Base32 => BASE32.encode(bytes),
            OutputFormat::Base32NoPad => BASE32_NOPAD.encode(bytes),
            OutputFormat::Base32Hex => BASE32HEX.encode(bytes),
            OutputFormat::Binary => {
                bytes.iter().map(|b| format!("{b:08b}")).collect()
            }
            OutputFormat::Decimal => bytes
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
}

impl FromStr for OutputFormat {
    type Err = HashError;

    fn from_str(s: &str) -> Result<Self> {
        let s = s.to_lowercase();
        OutputFormat::all()
            .into_iter()
            .find(|f| f.name() == s)
            .ok_or_else(|| {
                HashError::InvalidInput(format!(
                    "Unknown output format: {s}"
                ))
            })
    }
}

pub fn hash_file_raw(
    path: impl AsRef<Path>,
    algorithm: Algorithm,
) -> Result<Vec<u8>> {
    let hex = hashing::hash_file(path, algorithm)?;
    HEXLOWER.decode(hex.as_bytes()).map_err(|e| {
        HashError::InvalidInput(format!(
            "Unexpected digest encoding: {e}"
        ))
    })
}

pub fn hash_file_with_format(
    path: impl AsRef<Path>,
    algorithm: Algorithm,
    format: OutputFormat,
) -> Result<String> {
    Ok(format.encode(&hash_file_raw(path, algorithm)?))
}

pub fn hash(
    alg: &str,
    format: &str,
    path: impl AsRef<Path>,
) -> Result<String> {
    let alg = Algorithm::from_str(alg)?;
    let format = OutputFormat::from_str(format)?;
    hash_file_with_format(path, alg, format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_file(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "{}_{}",
            std::process::id(),
            name
        ));
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn formats() {
        let path = temp_file("hash_formats", b"hello world");

        let hex = hash("md5", "hex", &path).unwrap();
        assert_eq!(hex, "5eb63bbbe01eeed093cb22bb8f5acdc3");

        let upper = hash("md5", "hex-upper", &path).unwrap();
        assert_eq!(upper, hex.to_uppercase());

        let b64 = hash("md5", "base64", &path).unwrap();
        assert!(b64.ends_with("=="));

        let b64_nopad = hash("md5", "base64-nopad", &path).unwrap();
        assert!(!b64_nopad.contains('='));

        let bin = hash("md5", "binary", &path).unwrap();
        assert_eq!(bin.len(), 128);

        fs::remove_file(path).ok();
    }

    #[test]
    fn every_format_works_for_every_algorithm() {
        let path = temp_file("hash_all", b"test");

        for alg in Algorithm::all() {
            for fmt in OutputFormat::all() {
                let out = hash_file_with_format(&path, alg, fmt);
                assert!(out.is_ok(), "{alg:?} / {fmt:?} failed");
            }
        }

        fs::remove_file(path).ok();
    }

    #[test]
    fn invalid_inputs() {
        let path = temp_file("hash_invalid", b"x");

        assert!(hash("nope", "hex", &path).is_err());
        assert!(hash("sha256", "nope", &path).is_err());
        assert!(
            hash("sha256", "hex", "/definitely/not/a/file").is_err()
        );

        fs::remove_file(path).ok();
    }
}
