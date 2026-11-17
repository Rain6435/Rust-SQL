//! Self-updater for `Rust-SQL` using GitHub Releases, SHA-256 verification, and atomic binary replacement.

pub mod archive;

use crate::common::utils::version_compare;
use archive::{extract_binary_from_tar_gz, extract_binary_from_zip, verify_sha256};
use serde::Deserialize;
use tempfile::TempDir;

const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/Rain6435/Rust-SQL/releases/latest";

/// Metadata for an available `Rust-SQL` GitHub Release matching the host platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// Git tag name (e.g., `"v1.40.0"`).
    pub tag_name: String,
    /// Normalized semantic version without leading `v` (e.g., `"1.40.0"`).
    pub version: String,
    /// Web URL of the GitHub Release page.
    pub html_url: String,
    /// Archive asset filename for the current OS/architecture.
    pub asset_name: String,
    /// Direct download URL for the release archive.
    pub asset_url: String,
    /// Direct download URL for the `.sha256` checksum file, if published.
    pub sha256_url: Option<String>,
}

/// Outcome of checking GitHub Releases against the running binary version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheckResult {
    /// The running binary is already at or ahead of the latest published release.
    UpToDate {
        /// Current running version string.
        current: String,
    },
    /// A newer release is available on GitHub Releases.
    UpdateAvailable(ReleaseInfo),
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<GithubAsset>,
}

/// Maps the current `(OS, ARCH)` to the target triple used by `release.yml`.
pub fn current_target_triple() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        _ => None,
    }
}

/// Returns the expected release binary filename for the current platform (`Rust-SQL` or `Rust-SQL.exe`).
pub fn current_binary_name() -> &'static str {
    if cfg!(windows) {
        "Rust-SQL.exe"
    } else {
        "Rust-SQL"
    }
}

/// Strips a leading `'v'` or `'V'` prefix from a release tag.
pub fn normalize_version(tag: &str) -> &str {
    tag.trim().trim_start_matches(['v', 'V'])
}

fn build_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(format!("Rust-SQL/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

fn find_matching_assets(release: &GithubRelease, target: &str) -> Result<ReleaseInfo, String> {
    let ext = if target.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    let expected_suffix = format!("-{target}.{ext}");
    let expected_sha_suffix = format!("{expected_suffix}.sha256");

    let archive = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(&expected_suffix))
        .ok_or_else(|| {
            format!(
                "No release archive found for target '{target}' in {}",
                release.tag_name
            )
        })?;
    let sha_asset = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(&expected_sha_suffix));

    Ok(ReleaseInfo {
        tag_name: release.tag_name.clone(),
        version: normalize_version(&release.tag_name).to_string(),
        html_url: release.html_url.clone(),
        asset_name: archive.name.clone(),
        asset_url: archive.browser_download_url.clone(),
        sha256_url: sha_asset.map(|a| a.browser_download_url.clone()),
    })
}

/// Queries GitHub Releases for the latest version and compares it with `CARGO_PKG_VERSION`.
pub async fn check_for_updates() -> Result<UpdateCheckResult, String> {
    let target = current_target_triple().ok_or_else(|| {
        format!(
            "Self-update is not supported on {}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    let client = build_http_client()?;
    let resp = client
        .get(GITHUB_LATEST_RELEASE_URL)
        .send()
        .await
        .map_err(|e| format!("Failed to check for updates: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "GitHub Releases API returned HTTP {}",
            resp.status()
        ));
    }
    let release: GithubRelease = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse release metadata: {e}"))?;

    let current = env!("CARGO_PKG_VERSION").to_string();
    let latest_norm = normalize_version(&release.tag_name);
    if version_compare(latest_norm, &current) <= 0 {
        return Ok(UpdateCheckResult::UpToDate { current });
    }
    let info = find_matching_assets(&release, target)?;
    Ok(UpdateCheckResult::UpdateAvailable(info))
}

/// Downloads `info`, verifies its `.sha256` checksum if present, extracts the binary, and replaces the current executable.
pub async fn perform_self_update(info: &ReleaseInfo) -> Result<String, String> {
    let client = build_http_client()?;
    let archive_bytes = client
        .get(&info.asset_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download {}: {e}", info.asset_name))?
        .error_for_status()
        .map_err(|e| format!("Download error for {}: {e}", info.asset_name))?
        .bytes()
        .await
        .map_err(|e| format!("Failed to read archive bytes: {e}"))?;

    if let Some(ref sha_url) = info.sha256_url {
        let sha_text = client
            .get(sha_url)
            .send()
            .await
            .map_err(|e| format!("Failed to download checksum: {e}"))?
            .error_for_status()
            .map_err(|e| format!("Checksum download error: {e}"))?
            .text()
            .await
            .map_err(|e| format!("Failed to read checksum text: {e}"))?;
        verify_sha256(&archive_bytes, &sha_text)?;
    }

    let tmp = TempDir::new().map_err(|e| e.to_string())?;
    let bin_name = current_binary_name();
    let extracted_bin = if info.asset_name.ends_with(".zip") {
        extract_binary_from_zip(&archive_bytes, bin_name, tmp.path())?
    } else {
        extract_binary_from_tar_gz(&archive_bytes, bin_name, tmp.path())?
    };

    self_replace::self_replace(&extracted_bin).map_err(|e| {
        format!(
            "Failed to replace executable ({e}). If Rust-SQL is installed in a system or package-manager directory, upgrade via your package manager or download from {}",
            info.html_url
        )
    })?;

    Ok(format!(
        "Successfully updated Rust-SQL to {} ({})",
        info.tag_name, info.asset_name
    ))
}

/// Runs the synchronous CLI `--check-update` / `--update` flow and prints status to stdout.
pub fn run_update_cli(apply_update: bool) -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async {
        match check_for_updates().await? {
            UpdateCheckResult::UpToDate { current } => {
                println!("Rust-SQL v{current} is already up to date.");
                Ok(())
            }
            UpdateCheckResult::UpdateAvailable(info) if apply_update => {
                println!(
                    "Updating Rust-SQL to {} ({})...",
                    info.tag_name, info.asset_name
                );
                let msg = perform_self_update(&info).await?;
                println!("{msg}");
                Ok(())
            }
            UpdateCheckResult::UpdateAvailable(info) => {
                println!("Update available: {} ({})", info.tag_name, info.html_url);
                println!("Run `Rust-SQL --update` to install.");
                Ok(())
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use archive::{compute_sha256_hex, parse_sha256_digest};

    #[test]
    fn test_sha256_parse_and_verify() {
        let data = b"rust-sql-release-binary";
        let hex = compute_sha256_hex(data);
        let line = format!("{hex}  Rust-SQL-v1.40.0-x86_64-unknown-linux-gnu.tar.gz\n");
        assert_eq!(parse_sha256_digest(&line).unwrap(), hex);
        assert!(verify_sha256(data, &line).is_ok());
        assert!(verify_sha256(b"tampered", &line).is_err());
    }

    #[test]
    fn test_find_matching_assets_and_version_normalize() {
        assert_eq!(normalize_version("v1.40.0"), "1.40.0");
        let release = GithubRelease {
            tag_name: "v1.41.0".to_string(),
            html_url: "https://example.com/release".to_string(),
            assets: vec![
                GithubAsset {
                    name: "Rust-SQL-v1.41.0-x86_64-unknown-linux-gnu.tar.gz".to_string(),
                    browser_download_url: "https://example.com/linux.tar.gz".to_string(),
                },
                GithubAsset {
                    name: "Rust-SQL-v1.41.0-x86_64-unknown-linux-gnu.tar.gz.sha256".to_string(),
                    browser_download_url: "https://example.com/linux.tar.gz.sha256".to_string(),
                },
            ],
        };
        let info = find_matching_assets(&release, "x86_64-unknown-linux-gnu").unwrap();
        assert_eq!(info.version, "1.41.0");
        assert!(info.sha256_url.is_some());
    }
}
