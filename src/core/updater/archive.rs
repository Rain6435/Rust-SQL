//! SHA-256 verification and archive extraction helpers (`.tar.gz` and `.zip`) for self-updating.

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tar::Archive;
use zip::ZipArchive;

/// Parses the leading lowercase hex SHA-256 digest from a `.sha256` file (`<hex>  <filename>`).
pub fn parse_sha256_digest(checksum_text: &str) -> Result<String, String> {
    let token = checksum_text
        .split_whitespace()
        .next()
        .ok_or_else(|| "Empty SHA-256 checksum file".to_string())?;
    let normalized = token.trim().to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("Invalid SHA-256 digest format: '{token}'"));
    }
    Ok(normalized)
}

/// Computes the lowercase hex SHA-256 digest of `bytes`.
pub fn compute_sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Verifies that `archive_bytes` matches the digest in `checksum_text`.
pub fn verify_sha256(archive_bytes: &[u8], checksum_text: &str) -> Result<(), String> {
    let expected = parse_sha256_digest(checksum_text)?;
    let actual = compute_sha256_hex(archive_bytes);
    if actual != expected {
        return Err(format!(
            "SHA-256 checksum mismatch: expected {expected}, got {actual}"
        ));
    }
    Ok(())
}

/// Extracts `bin_name` from a `.tar.gz` archive into `dest_dir` and returns the extracted file path.
pub fn extract_binary_from_tar_gz(
    archive_bytes: &[u8],
    bin_name: &str,
    dest_dir: &Path,
) -> Result<PathBuf, String> {
    let gz = GzDecoder::new(Cursor::new(archive_bytes));
    let mut archive = Archive::new(gz);
    let entries = archive.entries().map_err(|e| e.to_string())?;
    for entry_res in entries {
        let mut entry = entry_res.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?;
        let is_target = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == bin_name)
            .unwrap_or(false);
        if is_target && entry.header().entry_type().is_file() {
            let out_path = dest_dir.join(bin_name);
            let mut out_file = File::create(&out_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out_file).map_err(|e| e.to_string())?;
            set_executable_permissions(&out_path)?;
            return Ok(out_path);
        }
    }
    Err(format!(
        "Binary '{bin_name}' not found inside .tar.gz archive"
    ))
}

/// Extracts `bin_name` from a `.zip` archive into `dest_dir` and returns the extracted file path.
pub fn extract_binary_from_zip(
    archive_bytes: &[u8],
    bin_name: &str,
    dest_dir: &Path,
) -> Result<PathBuf, String> {
    let mut zip = ZipArchive::new(Cursor::new(archive_bytes)).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
        if file.is_dir() {
            continue;
        }
        let is_target = Path::new(file.name())
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case(bin_name))
            .unwrap_or(false);
        if is_target {
            let out_path = dest_dir.join(bin_name);
            let mut out_file = File::create(&out_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut out_file).map_err(|e| e.to_string())?;
            set_executable_permissions(&out_path)?;
            return Ok(out_path);
        }
    }
    Err(format!("Binary '{bin_name}' not found inside .zip archive"))
}

#[cfg(unix)]
fn set_executable_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path).map_err(|e| e.to_string())?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn set_executable_permissions(_path: &Path) -> Result<(), String> {
    let _ = fs::metadata;
    Ok(())
}
