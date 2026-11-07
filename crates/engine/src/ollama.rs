use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    pub namespace: String,
    pub model_name: String,
    pub tag: String,
    pub total_size_bytes: u64,
    pub unique_blob_count: usize,
    pub deduplicated_layers_skipped: usize,
    pub raw_manifest_json: String,
    pub layers: Vec<OllamaResolvedBlob>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaResolvedBlob {
    pub file_name: String,
    pub blob_store_filename: String,
    pub role: String,
    pub media_type: String,
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
            .timeout(std::time::Duration::from_secs(20))
            .user_agent("otter-engine/0.1.0")
            .build()?;
        Ok(Self { client })
    }

    /// Query Ollama official registry for model manifest, deduplicating layers by SHA-256 digest.
    pub async fn fetch_manifest(&self, model_tag: &str) -> Result<OllamaResolvedModel> {
        let (namespace, name, tag) = parse_model_namespace_tag(model_tag);
        let url = format!(
            "https://registry.ollama.ai/v2/{}/{}/manifests/{}",
            namespace, name, tag
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
                "Ollama registry error (HTTP {}): Model '{}/{}:{}' not found",
                resp.status(),
                namespace,
                name,
                tag
            );
        }

        let bytes = resp.bytes().await?;
        let raw_manifest_json = String::from_utf8_lossy(&bytes).to_string();
        let manifest: OllamaManifest = serde_json::from_slice(&bytes)
            .context("Failed to parse Ollama registry manifest JSON")?;

        let mut blobs = Vec::new();
        let mut seen_digests = HashSet::new();
        let mut total_size_bytes = 0u64;
        let mut deduplicated_layers_skipped = 0usize;

        // Add config blob
        let cfg_clean = manifest.config.digest.replace("sha256:", "");
        seen_digests.insert(cfg_clean.clone());
        blobs.push(OllamaResolvedBlob {
            file_name: format!("sha256-{}", cfg_clean),
            blob_store_filename: format!("sha256-{}", cfg_clean),
            role: "Configuration (OCI Config)".to_string(),
            media_type: manifest.config.media_type.clone(),
            digest: cfg_clean.clone(),
            size_bytes: manifest.config.size,
            download_url: format!(
                "https://registry.ollama.ai/v2/{}/{}/blobs/sha256:{}",
                namespace, name, cfg_clean
            ),
        });
        total_size_bytes += manifest.config.size;

        // Add layer blobs with strict digest deduplication
        for layer in &manifest.layers {
            let digest_clean = layer.digest.replace("sha256:", "");
            if !seen_digests.insert(digest_clean.clone()) {
                deduplicated_layers_skipped += 1;
                continue;
            }

            let role = if layer.media_type.contains("model") {
                "Model Weights (GGUF)".to_string()
            } else if layer.media_type.contains("template") {
                "Prompt Template".to_string()
            } else if layer.media_type.contains("license") {
                "License".to_string()
            } else if layer.media_type.contains("params") {
                "Inference Parameters".to_string()
            } else if layer.media_type.contains("projector") {
                "Multimodal Projector".to_string()
            } else {
                "Binary Layer".to_string()
            };

            blobs.push(OllamaResolvedBlob {
                file_name: format!("sha256-{}", digest_clean),
                blob_store_filename: format!("sha256-{}", digest_clean),
                role,
                media_type: layer.media_type.clone(),
                digest: digest_clean.clone(),
                size_bytes: layer.size,
                download_url: format!(
                    "https://registry.ollama.ai/v2/{}/{}/blobs/sha256:{}",
                    namespace, name, digest_clean
                ),
            });
            total_size_bytes += layer.size;
        }

        let unique_blob_count = blobs.len();
        Ok(OllamaResolvedModel {
            namespace,
            model_name: name,
            tag,
            total_size_bytes,
            unique_blob_count,
            deduplicated_layers_skipped,
            raw_manifest_json,
            layers: blobs,
        })
    }

    /// Verify all downloaded blobs by SHA-256 and register the model in an isolated or target Ollama store.
    pub fn verify_and_register_store(
        resolved: &OllamaResolvedModel,
        downloaded_blobs_dir: &Path,
        target_ollama_store: &Path,
    ) -> Result<PathBuf> {
        let store_blobs_dir = target_ollama_store.join("blobs");
        let store_manifest_dir = target_ollama_store
            .join("manifests")
            .join("registry.ollama.ai")
            .join(&resolved.namespace)
            .join(&resolved.model_name);

        std::fs::create_dir_all(&store_blobs_dir)?;
        std::fs::create_dir_all(&store_manifest_dir)?;

        for blob in &resolved.layers {
            let src_path = downloaded_blobs_dir.join(&blob.blob_store_filename);
            let dst_path = store_blobs_dir.join(&blob.blob_store_filename);

            let check_path = if src_path.exists() {
                &src_path
            } else if dst_path.exists() {
                &dst_path
            } else {
                bail!(
                    "Missing required Ollama layer blob: {} (expected {} bytes)",
                    blob.blob_store_filename,
                    blob.size_bytes
                );
            };

            let meta = std::fs::metadata(check_path)?;
            if meta.len() != blob.size_bytes {
                bail!(
                    "Ollama blob size mismatch for {}: expected {}, got {}",
                    blob.blob_store_filename,
                    blob.size_bytes,
                    meta.len()
                );
            }

            let computed = crate::verify::compute_file_sha256(check_path)?;
            if !computed.eq_ignore_ascii_case(&blob.digest) {
                bail!(
                    "Ollama blob SHA-256 mismatch for {}: expected {}, got {}",
                    blob.blob_store_filename,
                    blob.digest,
                    computed
                );
            }

            if *check_path != dst_path {
                std::fs::copy(&src_path, &dst_path)?;
            }
        }

        let manifest_path = store_manifest_dir.join(&resolved.tag);
        std::fs::write(&manifest_path, resolved.raw_manifest_json.as_bytes())?;
        Ok(manifest_path)
    }
}

pub fn parse_model_tag(tag_str: &str) -> (String, String) {
    let (_, name, tag) = parse_model_namespace_tag(tag_str);
    (name, tag)
}

pub fn parse_model_namespace_tag(raw_input: &str) -> (String, String, String) {
    let mut s = raw_input.trim().to_string();
    // Strip CLI prefixes like "ollama pull " or "ollama run " or "ollama:"
    let lower = s.to_lowercase();
    if let Some(rest) = lower.strip_prefix("ollama pull ") {
        s = raw_input.trim()[12..].trim().to_string();
        let _ = rest;
    } else if let Some(rest) = lower.strip_prefix("ollama run ") {
        s = raw_input.trim()[11..].trim().to_string();
        let _ = rest;
    } else if lower.starts_with("ollama:") {
        s = raw_input.trim()[7..].trim().to_string();
    } else if lower.contains("ollama.com/") {
        if let Some(idx) = lower.find("ollama.com/") {
            s = raw_input.trim()[idx + 11..].trim().to_string();
        }
    }

    let clean = s.trim_matches('/');
    let (path_part, tag) = if let Some((p, t)) = clean.split_once(':') {
        (p.trim(), t.trim().to_string())
    } else {
        (clean, "latest".to_string())
    };

    if let Some((ns, name)) = path_part.split_once('/') {
        (ns.to_string(), name.to_string(), tag)
    } else {
        ("library".to_string(), path_part.to_string(), tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ollama_inputs() {
        assert_eq!(
            parse_model_namespace_tag("ollama pull qwen2.5:7b"),
            (
                "library".to_string(),
                "qwen2.5".to_string(),
                "7b".to_string()
            )
        );
        assert_eq!(
            parse_model_namespace_tag("ollama:deepseek-r1:1.5b"),
            (
                "library".to_string(),
                "deepseek-r1".to_string(),
                "1.5b".to_string()
            )
        );
        assert_eq!(
            parse_model_namespace_tag("https://ollama.com/library/llama3.2:1b"),
            (
                "library".to_string(),
                "llama3.2".to_string(),
                "1b".to_string()
            )
        );
    }
}
