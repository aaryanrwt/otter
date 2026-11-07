use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfLfsPointer {
    pub oid: String,
    pub size: u64,
    #[serde(rename = "pointerSize")]
    pub pointer_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfRepoInfo {
    pub id: String,
    pub author: Option<String>,
    pub sha: Option<String>,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub gated: serde_json::Value,
    pub downloads: Option<u64>,
    pub likes: Option<u64>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub siblings: Vec<HfFileSibling>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfFileSibling {
    pub rfilename: String,
    pub size: Option<u64>,
    pub blob_id: Option<String>,
    pub lfs: Option<HfLfsPointer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfResolvedDownloadItem {
    pub rfilename: String,
    pub download_url: String,
    pub expected_size: Option<u64>,
    pub expected_sha256: Option<String>,
}

pub struct HfClient {
    client: reqwest::Client,
    #[allow(dead_code)]
    token: Option<String>,
}

impl HfClient {
    pub fn new() -> Result<Self> {
        let token = std::env::var("HF_TOKEN")
            .or_else(|_| std::env::var("HUGGING_FACE_HUB_TOKEN"))
            .ok();

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static("otter-engine/0.1.0"),
        );
        if let Some(t) = &token
            && let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", t))
        {
            headers.insert(reqwest::header::AUTHORIZATION, val);
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(20))
            .build()?;

        Ok(Self { client, token })
    }

    /// Query Hugging Face Model metadata and file tree with `?blobs=true` for exact sizes and LFS SHA-256 digests
    pub async fn get_model_info(
        &self,
        model_id: &str,
        revision: Option<&str>,
    ) -> Result<HfRepoInfo> {
        let rev = revision.unwrap_or("main");
        let url = format!(
            "https://huggingface.co/api/models/{}?revision={}&blobs=true",
            model_id, rev
        );
        let resp =
            self.client.get(&url).send().await.with_context(|| {
                format!("Failed to reach Hugging Face API for model: {}", model_id)
            })?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED
            || resp.status() == reqwest::StatusCode::FORBIDDEN
        {
            bail!(
                "Authentication required or gated Hugging Face model (HTTP {}): '{}'. Set HF_TOKEN to access.",
                resp.status(),
                model_id
            );
        }

        if !resp.status().is_success() {
            bail!(
                "Hugging Face API error (HTTP {}): Model '{}' not found or inaccessible",
                resp.status(),
                model_id
            );
        }

        let bytes = resp.bytes().await?;
        let info: HfRepoInfo = serde_json::from_slice(&bytes)
            .context("Failed to parse Hugging Face model metadata JSON")?;
        Ok(info)
    }

    /// Query Hugging Face Dataset metadata and file tree with `?blobs=true` for exact sizes and LFS SHA-256 digests
    pub async fn get_dataset_info(
        &self,
        dataset_id: &str,
        revision: Option<&str>,
    ) -> Result<HfRepoInfo> {
        let rev = revision.unwrap_or("main");
        let url = format!(
            "https://huggingface.co/api/datasets/{}?revision={}&blobs=true",
            dataset_id, rev
        );
        let resp = self.client.get(&url).send().await.with_context(|| {
            format!(
                "Failed to reach Hugging Face API for dataset: {}",
                dataset_id
            )
        })?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED
            || resp.status() == reqwest::StatusCode::FORBIDDEN
        {
            bail!(
                "Authentication required or gated Hugging Face dataset (HTTP {}): '{}'. Set HF_TOKEN to access.",
                resp.status(),
                dataset_id
            );
        }

        if !resp.status().is_success() {
            bail!(
                "Hugging Face API error (HTTP {}): Dataset '{}' not found or inaccessible",
                resp.status(),
                dataset_id
            );
        }

        let bytes = resp.bytes().await?;
        let info: HfRepoInfo = serde_json::from_slice(&bytes)
            .context("Failed to parse Hugging Face dataset metadata JSON")?;
        Ok(info)
    }

    /// Build resolved download items for a model repo, including LFS size and SHA-256 digest
    pub fn build_model_download_list(
        &self,
        model_id: &str,
        info: &HfRepoInfo,
        revision: Option<&str>,
        filter_extension: Option<&str>,
    ) -> Vec<HfResolvedDownloadItem> {
        let rev = revision.unwrap_or("main");
        let mut items = Vec::new();

        for sibling in &info.siblings {
            let name = &sibling.rfilename;
            if name.starts_with(".git") || name == ".gitattributes" {
                continue;
            }

            if let Some(ext) = filter_extension
                && !name.ends_with(ext)
                && !is_essential_model_config(name)
            {
                continue;
            }

            let download_url = format!(
                "https://huggingface.co/{}/resolve/{}/{}",
                model_id, rev, name
            );

            let expected_size = sibling
                .size
                .or_else(|| sibling.lfs.as_ref().map(|l| l.size));
            let expected_sha256 = sibling.lfs.as_ref().map(|l| l.oid.clone());

            items.push(HfResolvedDownloadItem {
                rfilename: name.clone(),
                download_url,
                expected_size,
                expected_sha256,
            });
        }

        items
    }

    /// Build resolved download items for a dataset repo, including LFS size and SHA-256 digest
    pub fn build_dataset_download_list(
        &self,
        dataset_id: &str,
        info: &HfRepoInfo,
        revision: Option<&str>,
        subset: Option<&str>,
    ) -> Vec<HfResolvedDownloadItem> {
        let rev = revision.unwrap_or("main");
        let mut items = Vec::new();

        for sibling in &info.siblings {
            let name = &sibling.rfilename;
            if name.starts_with(".git") || name == ".gitattributes" {
                continue;
            }

            if let Some(sub) = subset
                && !name.contains(sub)
            {
                continue;
            }

            if !name.ends_with(".parquet")
                && !name.ends_with(".jsonl")
                && !name.ends_with(".arrow")
                && !name.ends_with(".csv")
                && !name.ends_with(".json")
                && !name.ends_with(".zip")
                && !name.ends_with(".tar.gz")
            {
                continue;
            }

            let download_url = format!(
                "https://huggingface.co/datasets/{}/resolve/{}/{}",
                dataset_id, rev, name
            );

            let expected_size = sibling
                .size
                .or_else(|| sibling.lfs.as_ref().map(|l| l.size));
            let expected_sha256 = sibling.lfs.as_ref().map(|l| l.oid.clone());

            items.push(HfResolvedDownloadItem {
                rfilename: name.clone(),
                download_url,
                expected_size,
                expected_sha256,
            });
        }

        items
    }
}

fn is_essential_model_config(file_name: &str) -> bool {
    file_name == "config.json"
        || file_name == "tokenizer.json"
        || file_name == "tokenizer_config.json"
        || file_name == "generation_config.json"
        || file_name == "special_tokens_map.json"
        || file_name.ends_with(".model")
}
