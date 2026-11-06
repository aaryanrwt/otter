use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaManifest {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub config: OllamaLayer,
    pub layers: Vec<OllamaLayer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaLayer {
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub digest: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaResolvedModel {
    pub model_name: String,
    pub tag: String,
    pub total_size_bytes: u64,
    pub layers: Vec<OllamaResolvedBlob>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaResolvedBlob {
    pub file_name: String,
    pub role: String,
    pub digest: String,
    pub size_bytes: u64,
    pub download_url: String,
}

pub struct OllamaRegistryClient {
    client: reqwest::Client,
}

impl OllamaRegistryClient {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("otter-engine/0.1.0")
            .build()?;
        Ok(Self { client })
    }

    /// Query Ollama official registry for model manifest
    pub async fn fetch_manifest(&self, model_tag: &str) -> Result<OllamaResolvedModel> {
        let (name, tag) = parse_model_tag(model_tag);
        let url = format!(
            "https://registry.ollama.ai/v2/library/{}/manifests/{}",
            name, tag
        );

        let resp = self
            .client
            .get(&url)
            .header(
                reqwest::header::ACCEPT,
                "application/vnd.docker.distribution.manifest.v2+json",
            )
            .send()
            .await
            .with_context(|| format!("Failed to connect to Ollama registry for {}", model_tag))?;

        if !resp.status().is_success() {
            bail!(
                "Ollama registry error (HTTP {}): Model '{}' not found",
                resp.status(),
                model_tag
            );
        }

        let bytes = resp.bytes().await?;
        let manifest: OllamaManifest = serde_json::from_slice(&bytes)
            .context("Failed to parse Ollama registry manifest JSON")?;

        let mut blobs = Vec::new();
        let mut total_size_bytes = 0u64;

        // Add config blob
        let cfg_clean = manifest.config.digest.replace("sha256:", "");
        blobs.push(OllamaResolvedBlob {
            file_name: "config.json".to_string(),
            role: "Configuration".to_string(),
            digest: cfg_clean.clone(),
            size_bytes: manifest.config.size,
            download_url: format!(
                "https://registry.ollama.ai/v2/library/{}/blobs/sha256:{}",
                name, cfg_clean
            ),
        });
        total_size_bytes += manifest.config.size;

        // Add layer blobs
        for layer in &manifest.layers {
            let digest_clean = layer.digest.replace("sha256:", "");
            let (file_name, role) = if layer.media_type.contains("model") {
                (
                    format!("{}-{}.Q4_K_M.gguf", name, tag),
                    "Model Weights (GGUF)".to_string(),
                )
            } else if layer.media_type.contains("template") {
                ("template.txt".to_string(), "Prompt Template".to_string())
            } else if layer.media_type.contains("license") {
                ("license.txt".to_string(), "License".to_string())
            } else if layer.media_type.contains("params") {
                (
                    "params.json".to_string(),
                    "Inference Parameters".to_string(),
                )
            } else {
                (
                    format!("blob_{}.bin", &digest_clean[..12.min(digest_clean.len())]),
                    "Binary Layer".to_string(),
                )
            };

            blobs.push(OllamaResolvedBlob {
                file_name,
                role,
                digest: digest_clean.clone(),
                size_bytes: layer.size,
                download_url: format!(
                    "https://registry.ollama.ai/v2/library/{}/blobs/sha256:{}",
                    name, digest_clean
                ),
            });
            total_size_bytes += layer.size;
        }

        Ok(OllamaResolvedModel {
            model_name: name,
            tag,
            total_size_bytes,
            layers: blobs,
        })
    }
}

pub fn parse_model_tag(tag_str: &str) -> (String, String) {
    let clean = tag_str.trim().trim_start_matches("library/");
    if let Some((name, tag)) = clean.split_once(':') {
        (name.to_string(), tag.to_string())
    } else {
        (clean.to_string(), "latest".to_string())
    }
}
