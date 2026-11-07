use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KaggleResolvedDataset {
    pub owner: String,
    pub slug: String,
    pub version: u32,
    pub archive_filename: String,
    pub api_download_url: String,
    pub signed_redirect_url: String,
    pub total_size_bytes: Option<u64>,
    pub resumable: bool,
    pub etag: Option<String>,
    pub auth_used: bool,
}

#[derive(Debug, Deserialize)]
struct KaggleJsonCreds {
    username: String,
    key: String,
}

pub struct KaggleClient {
    client: reqwest::Client,
    basic_auth: Option<(String, String)>,
}

impl KaggleClient {
    pub fn new() -> Result<Self> {
        let basic_auth = load_kaggle_credentials();
        let client = reqwest::Client::builder()
            .user_agent("kagglehub/0.3.0 otter-engine/0.1.0")
            .timeout(std::time::Duration::from_secs(20))
            .build()?;
        Ok(Self { client, basic_auth })
    }

    pub fn has_credentials(&self) -> bool {
        self.basic_auth.is_some()
    }

    /// Resolve a Kaggle dataset URL or CLI command into its downloadable archive metadata and signed GCS URL.
    pub async fn resolve_dataset(&self, raw_input: &str) -> Result<KaggleResolvedDataset> {
        let (owner, slug, version_opt) = parse_kaggle_dataset_input(raw_input)
            .ok_or_else(|| anyhow::anyhow!("Invalid Kaggle dataset identifier: {}", raw_input))?;
        let version = version_opt.unwrap_or(1);

        let api_download_url = format!(
            "https://www.kaggle.com/api/v1/datasets/download/{}/{}?datasetVersionNumber={}",
            owner, slug, version
        );

        let mut req = self
            .client
            .get(&api_download_url)
            .header(reqwest::header::RANGE, "bytes=0-0");

        let mut auth_used = false;
        if let Some((u, k)) = &self.basic_auth {
            req = req.basic_auth(u, Some(k));
            auth_used = true;
        }

        let resp = req
            .send()
            .await
            .with_context(|| format!("Failed to reach Kaggle API for {}/{}", owner, slug))?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            bail!(
                "Authentication required for Kaggle dataset '{}/{}' (HTTP {}). Configure KAGGLE_USERNAME/KAGGLE_KEY or ~/.kaggle/kaggle.json.",
                owner,
                slug,
                status
            );
        }
        if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
            bail!(
                "Kaggle dataset resolution error (HTTP {}) for '{}/{}'",
                status,
                owner,
                slug
            );
        }

        let signed_redirect_url = resp.url().to_string();
        let headers = resp.headers();

        // Parse total size from Content-Range (e.g. "bytes 0-0/18586941") or Content-Length
        let total_size_bytes = headers
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split('/').next_back())
            .and_then(|n| n.parse::<u64>().ok())
            .or_else(|| {
                headers
                    .get(reqwest::header::CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|n| n.parse::<u64>().ok())
            });

        let resumable = status == reqwest::StatusCode::PARTIAL_CONTENT
            || headers
                .get(reqwest::header::ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.contains("bytes"))
                .unwrap_or(false);

        let etag = headers
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        Ok(KaggleResolvedDataset {
            archive_filename: format!("{}.zip", slug),
            owner,
            slug,
            version,
            api_download_url,
            signed_redirect_url,
            total_size_bytes,
            resumable,
            etag,
            auth_used,
        })
    }
}

pub fn parse_kaggle_dataset_input(raw_input: &str) -> Option<(String, String, Option<u32>)> {
    let trimmed = raw_input.trim();
    let lower = trimmed.to_lowercase();

    // Case 1: CLI command `kaggle datasets download -d owner/slug` or `kaggle datasets download owner/slug`
    if lower.starts_with("kaggle ") {
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        for (idx, part) in parts.iter().enumerate() {
            if (*part == "-d" || *part == "--dataset") && idx + 1 < parts.len() {
                return split_owner_slug(parts[idx + 1]);
            }
        }
        if let Some(last) = parts.last()
            && last.contains('/')
        {
            return split_owner_slug(last);
        }
        return None;
    }

    // Case 2: `kaggle:owner/slug`
    if lower.starts_with("kaggle:") {
        return split_owner_slug(trimmed[7..].trim());
    }

    // Case 3: `https://www.kaggle.com/datasets/owner/slug` or `/api/v1/datasets/download/owner/slug`
    if lower.contains("kaggle.com/") {
        if let Some(idx) = lower.find("/datasets/download/") {
            let after = &trimmed[idx + 19..];
            let path_only = after.split('?').next().unwrap_or(after);
            return split_owner_slug(path_only);
        }
        if let Some(idx) = lower.find("/datasets/") {
            let after = &trimmed[idx + 10..];
            let path_only = after.split('?').next().unwrap_or(after);
            return split_owner_slug(path_only);
        }
    }

    None
}

fn split_owner_slug(s: &str) -> Option<(String, String, Option<u32>)> {
    let clean = s.trim().trim_matches('/');
    let segments: Vec<&str> = clean.split('/').collect();
    if segments.len() >= 2 && !segments[0].is_empty() && !segments[1].is_empty() {
        let owner = segments[0].to_string();
        let slug = segments[1].to_string();
        let ver = if segments.len() >= 4 && segments[2].eq_ignore_ascii_case("versions") {
            segments[3].parse::<u32>().ok()
        } else {
            None
        };
        Some((owner, slug, ver))
    } else {
        None
    }
}

fn load_kaggle_credentials() -> Option<(String, String)> {
    if let (Ok(u), Ok(k)) = (
        std::env::var("KAGGLE_USERNAME"),
        std::env::var("KAGGLE_KEY"),
    ) && !u.is_empty()
        && !k.is_empty()
    {
        return Some((u.trim().to_string(), k.trim().to_string()));
    }

    let user_profile = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    let cred_path = Path::new(&user_profile).join(".kaggle").join("kaggle.json");
    if cred_path.exists()
        && let Ok(content) = std::fs::read_to_string(&cred_path)
        && let Ok(creds) = serde_json::from_str::<KaggleJsonCreds>(&content)
    {
        return Some((
            creds.username.trim().to_string(),
            creds.key.trim().to_string(),
        ));
    }

    None
}

/// Verify that a downloaded Kaggle dataset `.zip` archive has a valid PKZIP header and End-of-Central-Directory record.
pub fn verify_kaggle_zip_archive(path: &Path) -> Result<(u64, String)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)
        .with_context(|| format!("Failed to open Kaggle archive: {}", path.display()))?;
    let len = f.metadata()?.len();
    if len < 22 {
        bail!("Kaggle archive is truncated ({} bytes < 22 bytes)", len);
    }

    let mut magic = [0u8; 4];
    f.read_exact(&mut magic)?;
    if &magic != b"PK\x03\x04" && &magic != b"PK\x05\x06" {
        bail!("Invalid ZIP magic header in {}", path.display());
    }

    // Verify End of Central Directory (EOCD) signature `PK\x05\x06` in the tail 65KB
    let tail_size = len.min(65557) as usize;
    f.seek(SeekFrom::End(-(tail_size as i64)))?;
    let mut tail = vec![0u8; tail_size];
    f.read_exact(&mut tail)?;
    let has_eocd = tail.windows(4).any(|w| w == b"PK\x05\x06");
    if !has_eocd {
        bail!(
            "Corrupt or incomplete ZIP archive (missing End-of-Central-Directory signature) in {}",
            path.display()
        );
    }

    let sha256 = crate::verify::compute_file_sha256(path)?;
    Ok((len, sha256))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_kaggle_inputs() {
        assert_eq!(
            parse_kaggle_dataset_input("https://www.kaggle.com/datasets/zynicide/wine-reviews"),
            Some(("zynicide".to_string(), "wine-reviews".to_string(), None))
        );
        assert_eq!(
            parse_kaggle_dataset_input("kaggle datasets download -d shivamb/netflix-shows"),
            Some(("shivamb".to_string(), "netflix-shows".to_string(), None))
        );
        assert_eq!(
            parse_kaggle_dataset_input("kaggle:PromptCloudHQ/imdb-data"),
            Some(("PromptCloudHQ".to_string(), "imdb-data".to_string(), None))
        );
    }
}
