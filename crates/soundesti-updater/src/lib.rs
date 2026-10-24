//! Update checking for Soundesti.
//!
//! The feed is a small JSON document:
//! `{"version":"0.2.0","url":"https://…/Soundesti Installer Wizard.exe","sha256":"…","notes":"…"}`.
//! A download is only accepted if its SHA-256 matches the feed. No data about the user
//! is sent; the request is a plain GET.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Official feed: the `update.json` asset of the latest GitHub release (GitHub redirects
/// `releases/latest/download/…` to the newest release, so the URL never changes).
pub const DEFAULT_FEED: Option<&str> =
    Some("https://github.com/Harel-k/soundesti/releases/latest/download/update.json");
/// Environment override, used for testing and for self-hosted feeds.
pub const FEED_ENV: &str = "SOUNDESTI_UPDATE_FEED";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub version: String,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("no update feed is configured yet")]
    NoFeed,
    #[error("network error: {0}")]
    Network(String),
    #[error("the update feed is invalid: {0}")]
    BadFeed(String),
    #[error("downloaded file failed verification (expected {expected}, got {actual})")]
    Checksum { expected: String, actual: String },
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("download cancelled")]
    Cancelled,
}

pub fn feed_url() -> Option<String> {
    std::env::var(FEED_ENV).ok().filter(|s| !s.trim().is_empty()).or(DEFAULT_FEED.map(str::to_string))
}

/// Parses and validates a feed document.
pub fn parse_feed(body: &str) -> Result<UpdateInfo, UpdateError> {
    let info: UpdateInfo = serde_json::from_str(body).map_err(|e| UpdateError::BadFeed(e.to_string()))?;
    semver::Version::parse(&info.version).map_err(|e| UpdateError::BadFeed(format!("version: {e}")))?;
    if info.sha256.len() != 64 || !info.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(UpdateError::BadFeed("sha256 must be 64 hex characters".into()));
    }
    if !(info.url.starts_with("https://")
        || info.url.starts_with("http://127.0.0.1")
        || info.url.starts_with("http://localhost"))
    {
        return Err(UpdateError::BadFeed("download URL must use HTTPS".into()));
    }
    Ok(info)
}

/// True if `candidate` is a newer version than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (semver::Version::parse(candidate), semver::Version::parse(current)) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}

fn get(url: &str) -> Result<ureq::Body, UpdateError> {
    ureq::get(url)
        .header("User-Agent", concat!("Soundesti-Updater/", env!("CARGO_PKG_VERSION")))
        .call()
        .map(|r| r.into_body())
        .map_err(|e| UpdateError::Network(e.to_string()))
}

/// Fetches the feed. Returns `Some` only when the feed offers a newer version than
/// `current` (or any version when `force` is set).
pub fn check(feed: &str, current: &str, force: bool) -> Result<Option<UpdateInfo>, UpdateError> {
    let body = get(feed)?.read_to_string().map_err(|e| UpdateError::Network(e.to_string()))?;
    let info = parse_feed(&body)?;
    Ok((force || is_newer(&info.version, current)).then_some(info))
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads the update into `dir`, verifying the checksum. `progress` gets bytes so
/// far and returns false to cancel. The file only appears at its final path once verified.
pub fn download(info: &UpdateInfo, dir: &Path, mut progress: impl FnMut(u64) -> bool) -> Result<PathBuf, UpdateError> {
    std::fs::create_dir_all(dir)?;
    let final_path = dir.join(format!("Soundesti-{}-setup.exe", info.version));
    let part = dir.join(format!("Soundesti-{}-setup.exe.part", info.version));
    let mut body = get(&info.url)?;
    let mut reader = body.as_reader();
    let mut out = std::fs::File::create(&part)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| UpdateError::Network(e.to_string()))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        h.update(&buf[..n]);
        total += n as u64;
        if !progress(total) {
            drop(out);
            let _ = std::fs::remove_file(&part);
            return Err(UpdateError::Cancelled);
        }
    }
    out.sync_all()?;
    drop(out);
    let actual = hex(&h.finalize());
    if !actual.eq_ignore_ascii_case(&info.sha256) {
        let _ = std::fs::remove_file(&part);
        return Err(UpdateError::Checksum { expected: info.sha256.clone(), actual });
    }
    std::fs::rename(&part, &final_path)?;
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// Minimal one-shot-per-connection HTTP server serving fixed routes.
    fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in l.incoming().flatten() {
                let mut s = stream;
                let mut req = [0u8; 2048];
                let n = s.read(&mut req).unwrap_or(0);
                let line = String::from_utf8_lossy(&req[..n]).lines().next().unwrap_or_default().to_string();
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                match routes.iter().find(|(p, _)| *p == path) {
                    Some((_, body)) => {
                        let _ =
                            write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                        let _ = s.write_all(body);
                    }
                    None => {
                        let _ = write!(s, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                }
            }
        });
        format!("http://127.0.0.1:{}", addr.port())
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("soundesti-upd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn version_comparison() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    /// Talks to the real GitHub release feed: `cargo test -p soundesti-updater -- --ignored`.
    #[test]
    #[ignore]
    fn live_github_feed() {
        let feed = DEFAULT_FEED.expect("official feed configured");
        let info = check(feed, "0.0.1", false).expect("feed reachable").expect("any release is newer than 0.0.1");
        assert!(info.url.starts_with("https://github.com/Harel-k/soundesti/releases/download/"));
        let dir = temp("live");
        let path = download(&info, &dir, |_| true).expect("download verifies against the feed's SHA-256");
        assert!(std::fs::metadata(&path).unwrap().len() > 1_000_000);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn feed_validation() {
        let good = format!(r#"{{"version":"1.2.3","url":"https://x/y.exe","sha256":"{}"}}"#, "a".repeat(64));
        assert!(parse_feed(&good).is_ok());
        let http = format!(r#"{{"version":"1.2.3","url":"http://evil/y.exe","sha256":"{}"}}"#, "a".repeat(64));
        assert!(matches!(parse_feed(&http), Err(UpdateError::BadFeed(_))));
        let bad_hash = r#"{"version":"1.2.3","url":"https://x/y.exe","sha256":"zz"}"#;
        assert!(matches!(parse_feed(bad_hash), Err(UpdateError::BadFeed(_))));
        assert!(matches!(parse_feed("{"), Err(UpdateError::BadFeed(_))));
    }

    #[test]
    fn check_and_download_verified() {
        let payload = b"pretend installer bytes".to_vec();
        let hash = sha256_hex(&payload);
        // Two-phase: start server with the payload, then serve a feed pointing at it.
        let base = serve(vec![("/setup.exe", payload.clone())]);
        let feed = format!(r#"{{"version":"9.0.0","url":"{base}/setup.exe","sha256":"{hash}","notes":"n"}}"#);
        let feed_base = serve(vec![("/feed.json", feed.into_bytes())]);
        let info = check(&format!("{feed_base}/feed.json"), "0.1.0", false).unwrap().expect("newer version");
        assert_eq!(info.version, "9.0.0");
        assert!(check(&format!("{feed_base}/feed.json"), "9.0.0", false).unwrap().is_none());
        assert!(check(&format!("{feed_base}/feed.json"), "9.0.0", true).unwrap().is_some());
        let dir = temp("dl");
        let path = download(&info, &dir, |_| true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), payload);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn tampered_download_is_rejected() {
        let base = serve(vec![("/setup.exe", b"tampered".to_vec())]);
        let info = UpdateInfo {
            version: "9.0.0".into(),
            url: format!("{base}/setup.exe"),
            sha256: "0".repeat(64),
            notes: String::new(),
        };
        let dir = temp("bad");
        assert!(matches!(download(&info, &dir, |_| true), Err(UpdateError::Checksum { .. })));
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "no file may remain");
        std::fs::remove_dir_all(dir).ok();
    }
}
