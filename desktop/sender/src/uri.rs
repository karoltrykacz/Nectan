use std::path::{Path, PathBuf};
use url::Url;

pub fn file_uri(path: &Path) -> String {
    let mut uri = Url::from_file_path(path)
        .expect("path must be an absolute filesystem path")
        .to_string();
    uri.push_str("\r\n");
    uri
}

/// Decode a text/uri-list payload into local filesystem paths.
pub fn decode_uri_list(input: &str) -> Vec<PathBuf> {
    input
        .lines()
        .filter_map(|line| {
            let line = line.trim();

            if line.is_empty() || line.starts_with('#') {
                return None;
            }

            let uri = Url::parse(line).ok()?;

            if uri.scheme() != "file" {
                return None;
            }

            // Do not accept remote file hosts.
            if let Some(host) = uri.host_str() {
                if !host.is_empty() && host != "localhost" {
                    return None;
                }
            }

            let path = uri.to_file_path().ok()?;

            if path.is_absolute() { Some(path) } else { None }
        })
        .collect()
}
