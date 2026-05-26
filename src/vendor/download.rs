//! Streaming wheel download with sha256 verification.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::VendorError;

pub fn download_wheel(
    url: &url::Url,
    dest: &Path,
    package: &str,
    version: &str,
    expected_sha256: &str,
) -> Result<(), VendorError> {
    let response = reqwest::blocking::get(url.clone())
        .map_err(|e| boxed_download(package, version, url, Box::new(e)))?
        .error_for_status()
        .map_err(|e| boxed_download(package, version, url, Box::new(e)))?;

    let parent = dest.parent().expect("dest must have a parent");
    fs::create_dir_all(parent).map_err(|e| boxed_download(package, version, url, Box::new(e)))?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        dest.file_name().unwrap().to_string_lossy()
    ));

    let result = (|| -> Result<(), VendorError> {
        let mut file = fs::File::create(&tmp)
            .map_err(|e| boxed_download(package, version, url, Box::new(e)))?;
        let mut hasher = Sha256::new();
        let mut reader = response;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| boxed_download(package, version, url, Box::new(e)))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])
                .map_err(|e| boxed_download(package, version, url, Box::new(e)))?;
        }
        file.sync_all()
            .map_err(|e| boxed_download(package, version, url, Box::new(e)))?;
        let actual = hex::encode(hasher.finalize());
        let expected = expected_sha256.trim_start_matches("sha256:");
        if actual != expected {
            return Err(VendorError::HashMismatch {
                package: package.into(),
                version: version.into(),
                expected: expected.into(),
                actual,
            });
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            fs::rename(&tmp, dest)
                .map_err(|e| boxed_download(package, version, url, Box::new(e)))?;
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            let _ = fs::remove_file(dest);
            Err(e)
        }
    }
}

fn boxed_download(
    package: &str,
    version: &str,
    url: &url::Url,
    source: Box<dyn std::error::Error + Send + Sync>,
) -> VendorError {
    VendorError::Download {
        package: package.into(),
        version: version.into(),
        url: url.to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spin up a tiny HTTP server that serves a fixed body once.
    fn serve_once(body: Vec<u8>) -> (url::Url, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::Read;
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        let url = url::Url::parse(&format!("http://{}/wheel.whl", addr)).unwrap();
        (url, handle)
    }

    fn serve_404() -> (url::Url, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::Read;
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            }
        });
        let url = url::Url::parse(&format!("http://{}/wheel.whl", addr)).unwrap();
        (url, handle)
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(bytes);
        hex::encode(h.finalize())
    }

    #[test]
    fn download_writes_file_when_sha_matches() {
        let body = b"hello-wheel-bytes".to_vec();
        let sha = sha256_hex(&body);
        let (url, h) = serve_once(body.clone());
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("wheel.whl");
        download_wheel(&url, &dest, "pkg", "1.0", &sha).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        let _ = h.join();
    }

    #[test]
    fn download_aborts_on_sha_mismatch_and_removes_partial() {
        let body = b"some-bytes".to_vec();
        let wrong_sha = "00".repeat(32);
        let (url, h) = serve_once(body);
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("wheel.whl");
        let err = download_wheel(&url, &dest, "pkg", "1.0", &wrong_sha).unwrap_err();
        match err {
            VendorError::HashMismatch { package, .. } => assert_eq!(package, "pkg"),
            other => panic!("expected HashMismatch, got {other:?}"),
        }
        assert!(!dest.exists(), "partial file should be removed");
        let _ = h.join();
    }

    #[test]
    fn download_aborts_on_404_and_removes_partial() {
        let (url, h) = serve_404();
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("wheel.whl");
        let err = download_wheel(&url, &dest, "pkg", "1.0", &"0".repeat(64)).unwrap_err();
        match err {
            VendorError::Download { package, .. } => assert_eq!(package, "pkg"),
            other => panic!("expected Download, got {other:?}"),
        }
        assert!(!dest.exists(), "partial file should be removed");
        let _ = h.join();
    }
}
