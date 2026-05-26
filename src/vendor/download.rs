//! Streaming wheel download with sha256 verification.

use std::path::Path;

use crate::error::VendorError;

/// Download `url` to `dest`, streaming bytes through a sha256 hasher.
/// On success: `dest` contains the wheel bytes; returned hex sha matches.
/// On failure: `dest` is removed (no partial files); error names the package.
pub fn download_wheel(
    _url: &url::Url,
    _dest: &Path,
    _package: &str,
    _version: &str,
    _expected_sha256: &str,
) -> Result<(), VendorError> {
    unimplemented!("Task 4")
}
