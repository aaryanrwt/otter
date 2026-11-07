use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlatformKind {
    YouTube,
    Instagram,
    Pinterest,
    TikTok,
    HuggingFace,
    Ollama,
    Kaggle,
    Steam,
    EpicGames,
    GitHub,
    DirectFile,
    GenericWeb,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentKind {
    Video,
    Audio,
    Image,
    Archive,
    Document,
    AiModel,
    AiDataset,
    Game,
    Software,
    GenericFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadVariant {
    pub label: String,
    pub quality: Option<String>,
    pub format: String,
    pub approx_size_bytes: Option<u64>,
    pub download_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    pub raw_url: String,
    pub is_valid: bool,
    pub platform: PlatformKind,
    pub platform_name: String,
    pub platform_icon: String,
    pub content_kind: ContentKind,
    pub content_type_label: String,
    pub title: String,
    pub suggested_filename: String,
    pub expected_size_bytes: Option<u64>,
    pub suggested_folder: String,
    pub requires_auth: bool,
    pub auth_note: Option<String>,
    pub client_handoff_protocol: Option<String>,
    pub variants: Vec<DownloadVariant>,
    pub error_message: Option<String>,
}

pub struct UrlDetector;

impl UrlDetector {
    /// Security validation: block localhost, private networks, SSRF
    pub fn validate_security(url_str: &str) -> Result<Url, String> {
        let trimmed = url_str.trim();
        if trimmed.is_empty() {
            return Err("URL is empty".into());
        }

        let lower_trim = trimmed.to_lowercase();

        // Support CLI command forms: `ollama pull <model>:<tag>`, `kaggle datasets download -d <owner>/<slug>`
        if lower_trim.starts_with("ollama pull ") || lower_trim.starts_with("ollama run ") {
            let model_part = trimmed.split_whitespace().nth(2).unwrap_or("qwen2.5:7b");
            let fake_http = format!("https://ollama.com/library/{}", model_part);
            if let Ok(parsed) = Url::parse(&fake_http) {
                return Ok(parsed);
            }
        }
        if lower_trim.starts_with("kaggle ") {
            if let Some((owner, slug, _)) = engine::kaggle::parse_kaggle_dataset_input(trimmed) {
                let fake_http = format!("https://www.kaggle.com/datasets/{}/{}", owner, slug);
                if let Ok(parsed) = Url::parse(&fake_http) {
                    return Ok(parsed);
                }
            }
        }

        // Support custom protocol prefixes like ollama:tag, hf:repo, kaggle:owner/slug
        if lower_trim.starts_with("ollama:")
            || lower_trim.starts_with("hf:")
            || lower_trim.starts_with("kaggle:")
            || lower_trim.starts_with("steam:")
        {
            if lower_trim.starts_with("ollama:") {
                let tag = trimmed[7..].trim();
                if let Ok(parsed) = Url::parse(&format!("https://ollama.com/library/{}", tag)) {
                    return Ok(parsed);
                }
            }
            if lower_trim.starts_with("kaggle:") {
                let slug = trimmed[7..].trim();
                if let Ok(parsed) = Url::parse(&format!("https://www.kaggle.com/datasets/{}", slug))
                {
                    return Ok(parsed);
                }
            }
            let fake_http = format!("https://{}", trimmed.replace(':', "/"));
            if let Ok(parsed) = Url::parse(&fake_http) {
                return Ok(parsed);
            }
        }

        let parsed = Url::parse(trimmed).map_err(|e| format!("Invalid URL syntax: {}", e))?;

        let scheme = parsed.scheme().to_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(format!(
                "Unsupported protocol '{}'. Only HTTP and HTTPS are permitted.",
                scheme
            ));
        }

        if let Some(host_str) = parsed.host_str() {
            let lower_host = host_str.to_lowercase();

            // Block localhost and loopback names
            if lower_host == "localhost"
                || lower_host.ends_with(".localhost")
                || lower_host == "127.0.0.1"
                || lower_host == "::1"
            {
                return Err(
                    "Security check failed: Localhost and loopback access are prohibited.".into(),
                );
            }

            // Check IP address subnets
            if let Ok(ip) = host_str.parse::<IpAddr>() {
                match ip {
                    IpAddr::V4(v4) => {
                        if v4.is_loopback()
                            || v4.is_private()
                            || v4.is_link_local()
                            || v4.is_broadcast()
                            || v4.is_documentation()
                        {
                            return Err(format!(
                                "Security check failed: IP {} is in a private or restricted network range.",
                                v4
                            ));
                        }
                    }
                    IpAddr::V6(v6) => {
                        if v6.is_loopback() {
                            return Err(
                                "Security check failed: IPv6 loopback is prohibited.".into()
                            );
                        }
                    }
                }
            }
        } else {
            return Err("URL does not contain a valid host name.".into());
        }

        Ok(parsed)
    }

    /// Full universal URL detection and source categorization
    pub fn detect(url_str: &str, default_download_dir: &str) -> DetectionResult {
        let parsed = match Self::validate_security(url_str) {
            Ok(p) => p,
            Err(e) => {
                return DetectionResult {
                    raw_url: url_str.to_string(),
                    is_valid: false,
                    platform: PlatformKind::GenericWeb,
                    platform_name: "Invalid / Blocked".to_string(),
                    platform_icon: "alert-circle".to_string(),
                    content_kind: ContentKind::GenericFile,
                    content_type_label: "Blocked URL".to_string(),
                    title: "Security or Validation Error".to_string(),
                    suggested_filename: "blocked.bin".to_string(),
                    expected_size_bytes: None,
                    suggested_folder: default_download_dir.to_string(),
                    requires_auth: false,
                    auth_note: None,
                    client_handoff_protocol: None,
                    variants: Vec::new(),
                    error_message: Some(e),
                };
            }
        };

        let host = parsed.host_str().unwrap_or("").to_lowercase();
        let path = parsed.path().to_lowercase();
        let path_segments: Vec<&str> = parsed
            .path_segments()
            .map(|s| s.collect())
            .unwrap_or_default();

        // 1. YouTube
        if host.contains("youtube.com") || host.contains("youtu.be") {
            let video_id = if host.contains("youtu.be") {
                path_segments
                    .first()
                    .copied()
                    .unwrap_or("video")
                    .to_string()
            } else {
                parsed
                    .query_pairs()
                    .find(|(k, _)| k == "v")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or_else(|| "video".to_string())
            };
            let title = format!("YouTube Video ({})", &video_id[..8.min(video_id.len())]);
            return DetectionResult {
                raw_url: url_str.to_string(),
                is_valid: true,
                platform: PlatformKind::YouTube,
                platform_name: "YouTube".to_string(),
                platform_icon: "video".to_string(),
                content_kind: ContentKind::Video,
                content_type_label: "Video / Audio Stream".to_string(),
                title: title.clone(),
                suggested_filename: format!("{}.mp4", title.replace(' ', "_")),
                expected_size_bytes: None,
                suggested_folder: format!("{}\\Videos", default_download_dir),
                requires_auth: false,
                auth_note: None,
                client_handoff_protocol: None,
                variants: vec![
                    DownloadVariant {
                        label: "Best Quality Video (1080p MP4)".into(),
                        quality: Some("1080p".into()),
                        format: "mp4".into(),
                        approx_size_bytes: None,
                        download_url: url_str.to_string(),
                    },
                    DownloadVariant {
                        label: "Audio Only (High Quality MP3 / M4A)".into(),
                        quality: Some("320kbps".into()),
                        format: "m4a".into(),
                        approx_size_bytes: None,
                        download_url: url_str.to_string(),
                    },
                ],
                error_message: None,
            };
        }

        // 2. Instagram
        if host.contains("instagram.com") {
            let is_reel = path.contains("/reel/") || path.contains("/reels/");
            return DetectionResult {
                raw_url: url_str.to_string(),
                is_valid: true,
                platform: PlatformKind::Instagram,
                platform_name: "Instagram".to_string(),
                platform_icon: "camera".to_string(),
                content_kind: if is_reel {
                    ContentKind::Video
                } else {
                    ContentKind::Image
                },
                content_type_label: if is_reel {
                    "Instagram Reel".to_string()
                } else {
                    "Instagram Media".to_string()
                },
                title: "Instagram Post Media".to_string(),
                suggested_filename: if is_reel {
                    "instagram_reel.mp4".to_string()
                } else {
                    "instagram_photo.jpg".to_string()
                },
                expected_size_bytes: None,
                suggested_folder: format!("{}\\Media", default_download_dir),
                requires_auth: false,
                auth_note: Some("Private accounts require active session authorization.".into()),
                client_handoff_protocol: None,
                variants: vec![DownloadVariant {
                    label: "Original Media (MP4 / JPG)".into(),
                    quality: Some("Highest".into()),
                    format: if is_reel { "mp4".into() } else { "jpg".into() },
                    approx_size_bytes: None,
                    download_url: url_str.to_string(),
                }],
                error_message: None,
            };
        }

        // 3. Pinterest
        if host.contains("pinterest.com") || host.contains("pinimg.com") {
            let is_video = path.ends_with(".mp4") || path.contains("/video/");
            return DetectionResult {
                raw_url: url_str.to_string(),
                is_valid: true,
                platform: PlatformKind::Pinterest,
                platform_name: "Pinterest".to_string(),
                platform_icon: "image".to_string(),
                content_kind: if is_video {
                    ContentKind::Video
                } else {
                    ContentKind::Image
                },
                content_type_label: if is_video {
                    "Pinterest Video Pin".to_string()
                } else {
                    "Pinterest Image Pin".to_string()
                },
                title: "Pinterest Visual Pin".to_string(),
                suggested_filename: if is_video {
                    "pinterest_pin.mp4".to_string()
                } else {
                    "pinterest_pin.jpg".to_string()
                },
                expected_size_bytes: None,
                suggested_folder: format!("{}\\Images", default_download_dir),
                requires_auth: false,
                auth_note: None,
                client_handoff_protocol: None,
                variants: vec![DownloadVariant {
                    label: "Full Resolution Asset".into(),
                    quality: Some("Original".into()),
                    format: if is_video { "mp4".into() } else { "jpg".into() },
                    approx_size_bytes: None,
                    download_url: url_str.to_string(),
                }],
                error_message: None,
            };
        }

        // 4. Ollama Registry & CLI (`ollama pull model:tag`)
        let lower_raw = url_str.trim().to_lowercase();
        if host.contains("ollama.com")
            || host.contains("ollama.ai")
            || lower_raw.starts_with("ollama:")
            || lower_raw.starts_with("ollama pull ")
            || lower_raw.starts_with("ollama run ")
        {
            let (ns, name, tag) = engine::ollama::parse_model_namespace_tag(url_str);
            let tag_display = if ns == "library" {
                format!("{}:{}", name, tag)
            } else {
                format!("{}/{}:{}", ns, name, tag)
            };
            return DetectionResult {
                raw_url: url_str.to_string(),
                is_valid: true,
                platform: PlatformKind::Ollama,
                platform_name: "Ollama Registry".to_string(),
                platform_icon: "cpu".to_string(),
                content_kind: ContentKind::AiModel,
                content_type_label: "Ollama Multi-Layer Model (Manifest + Blobs)".to_string(),
                title: format!("Ollama Model: {}", tag_display),
                suggested_filename: tag_display.replace(':', "_").replace('/', "_"),
                expected_size_bytes: None,
                suggested_folder: format!("{}\\Models\\Ollama", default_download_dir),
                requires_auth: false,
                auth_note: Some("Public Ollama library model. All OCI layers verified by content-addressed SHA-256 digest.".into()),
                client_handoff_protocol: None,
                variants: vec![
                    DownloadVariant {
                        label: format!("Full Ollama Model Store Layout ({})", tag_display),
                        quality: Some("Content-Addressed OCI Layers".into()),
                        format: "ollama-store".into(),
                        approx_size_bytes: None,
                        download_url: url_str.to_string(),
                    }
                ],
                error_message: None,
            };
        }

        // 5. Hugging Face (Model Repo, Dataset Repo, or Single File)
        if host.contains("huggingface.co") || lower_raw.starts_with("hf:") {
            let is_dataset =
                path.starts_with("/datasets/") || lower_raw.starts_with("hf:datasets/");
            let is_single_file = path.contains("/resolve/") || path.contains("/blob/");
            let repo_id = if is_dataset {
                if path_segments.len() >= 3 {
                    format!("{}/{}", path_segments[1], path_segments[2])
                } else {
                    path_segments
                        .last()
                        .copied()
                        .unwrap_or("dataset")
                        .to_string()
                }
            } else if path_segments.len() >= 2 {
                format!("{}/{}", path_segments[0], path_segments[1])
            } else {
                path_segments.last().copied().unwrap_or("model").to_string()
            };
            let clean_name = if is_single_file {
                path_segments
                    .last()
                    .copied()
                    .unwrap_or("artifact.bin")
                    .to_string()
            } else {
                repo_id.replace('/', "__")
            };
            return DetectionResult {
                raw_url: url_str.to_string(),
                is_valid: true,
                platform: PlatformKind::HuggingFace,
                platform_name: "Hugging Face".to_string(),
                platform_icon: "box".to_string(),
                content_kind: if is_dataset {
                    ContentKind::AiDataset
                } else {
                    ContentKind::AiModel
                },
                content_type_label: if is_dataset {
                    "Hugging Face Dataset Repository".to_string()
                } else if is_single_file {
                    "Hugging Face Model Artifact".to_string()
                } else {
                    "Hugging Face Model Repository".to_string()
                },
                title: format!(
                    "Hugging Face: {}",
                    if is_single_file {
                        &clean_name
                    } else {
                        &repo_id
                    }
                ),
                suggested_filename: clean_name.clone(),
                expected_size_bytes: None,
                suggested_folder: if is_dataset {
                    format!("{}\\Datasets\\HuggingFace", default_download_dir)
                } else {
                    format!("{}\\Models\\HuggingFace", default_download_dir)
                },
                requires_auth: false,
                auth_note: Some(
                    "Public repositories require no token. Private/gated repos use HF_TOKEN."
                        .into(),
                ),
                client_handoff_protocol: None,
                variants: vec![DownloadVariant {
                    label: format!("Verified Repository / Shard Transfer ({})", clean_name),
                    quality: Some("LFS SHA-256 Verified".into()),
                    format: clean_name.rsplit('.').next().unwrap_or("repo").to_string(),
                    approx_size_bytes: None,
                    download_url: url_str.to_string(),
                }],
                error_message: None,
            };
        }

        // 5b. Kaggle Datasets (`https://www.kaggle.com/datasets/owner/slug` or `kaggle datasets download -d owner/slug`)
        if host.contains("kaggle.com")
            || lower_raw.starts_with("kaggle:")
            || lower_raw.starts_with("kaggle ")
        {
            let (owner, slug, _ver) = engine::kaggle::parse_kaggle_dataset_input(url_str)
                .unwrap_or_else(|| ("kaggle".to_string(), "dataset".to_string(), None));
            let archive_name = format!("{}.zip", slug);
            return DetectionResult {
                raw_url: url_str.to_string(),
                is_valid: true,
                platform: PlatformKind::Kaggle,
                platform_name: "Kaggle Datasets".to_string(),
                platform_icon: "database".to_string(),
                content_kind: ContentKind::AiDataset,
                content_type_label: "Kaggle Dataset Archive (ZIP)".to_string(),
                title: format!("Kaggle Dataset: {}/{}", owner, slug),
                suggested_filename: archive_name.clone(),
                expected_size_bytes: None,
                suggested_folder: format!("{}\\Datasets\\Kaggle", default_download_dir),
                requires_auth: false,
                auth_note: Some("Public Kaggle datasets resolve via GCS signed URLs; restricted datasets use KAGGLE_USERNAME/KAGGLE_KEY or ~/.kaggle/kaggle.json.".into()),
                client_handoff_protocol: None,
                variants: vec![
                    DownloadVariant {
                        label: format!("Dataset ZIP Archive ({})", archive_name),
                        quality: Some("PKZIP + SHA-256 Verified".into()),
                        format: "zip".into(),
                        approx_size_bytes: None,
                        download_url: url_str.to_string(),
                    }
                ],
                error_message: None,
            };
        }

        // 6. Steam Store
        if host.contains("steampowered.com") || host.contains("steamcommunity.com") {
            let app_id = path_segments
                .iter()
                .position(|&s| s == "app")
                .and_then(|idx| path_segments.get(idx + 1))
                .copied()
                .unwrap_or("unknown");
            return DetectionResult {
                raw_url: url_str.to_string(),
                is_valid: true,
                platform: PlatformKind::Steam,
                platform_name: "Steam".to_string(),
                platform_icon: "gamepad".to_string(),
                content_kind: ContentKind::Game,
                content_type_label: "Steam Game Product".to_string(),
                title: format!("Steam App #{}", app_id),
                suggested_filename: format!("steam_app_{}.url", app_id),
                expected_size_bytes: None,
                suggested_folder: format!("{}\\Games", default_download_dir),
                requires_auth: true,
                auth_note: Some(
                    "Requires licensed Steam account ownership. Handoff to Steam client provided."
                        .into(),
                ),
                client_handoff_protocol: Some(format!("steam://run/{}", app_id)),
                variants: vec![DownloadVariant {
                    label: "Official Steam Client Installation".into(),
                    quality: Some("Licensed".into()),
                    format: "steam-protocol".into(),
                    approx_size_bytes: None,
                    download_url: format!("steam://install/{}", app_id),
                }],
                error_message: None,
            };
        }

        // 7. Epic Games Store
        if host.contains("epicgames.com") {
            return DetectionResult {
                raw_url: url_str.to_string(),
                is_valid: true,
                platform: PlatformKind::EpicGames,
                platform_name: "Epic Games".to_string(),
                platform_icon: "gamepad".to_string(),
                content_kind: ContentKind::Game,
                content_type_label: "Epic Games Store Product".to_string(),
                title: "Epic Games Store Title".to_string(),
                suggested_filename: "epic_game.url".to_string(),
                expected_size_bytes: None,
                suggested_folder: format!("{}\\Games", default_download_dir),
                requires_auth: true,
                auth_note: Some("Requires official Epic Games Launcher entitlement.".into()),
                client_handoff_protocol: Some("com.epicgames.launcher://".to_string()),
                variants: vec![DownloadVariant {
                    label: "Epic Games Launcher Handoff".into(),
                    quality: Some("Official".into()),
                    format: "epic-protocol".into(),
                    approx_size_bytes: None,
                    download_url: url_str.to_string(),
                }],
                error_message: None,
            };
        }

        // 8. GitHub Releases / Repositories
        if host.contains("github.com") {
            let last = path_segments.last().copied().unwrap_or("asset.zip");
            let is_release = path.contains("/releases/download/");
            return DetectionResult {
                raw_url: url_str.to_string(),
                is_valid: true,
                platform: PlatformKind::GitHub,
                platform_name: "GitHub".to_string(),
                platform_icon: "github".to_string(),
                content_kind: if is_release {
                    ContentKind::Software
                } else {
                    ContentKind::Archive
                },
                content_type_label: if is_release {
                    "Release Binary".to_string()
                } else {
                    "Repository Archive".to_string()
                },
                title: format!("GitHub: {}", last),
                suggested_filename: last.to_string(),
                expected_size_bytes: None,
                suggested_folder: format!("{}\\Downloads", default_download_dir),
                requires_auth: false,
                auth_note: None,
                client_handoff_protocol: None,
                variants: vec![DownloadVariant {
                    label: format!("Direct Release Download ({})", last),
                    quality: Some("Release".into()),
                    format: last.rsplit('.').next().unwrap_or("bin").to_string(),
                    approx_size_bytes: None,
                    download_url: url_str.to_string(),
                }],
                error_message: None,
            };
        }

        // 9. Direct File by Extension
        let raw_filename = path_segments.last().copied().unwrap_or("download.bin");
        let filename = if raw_filename.is_empty() {
            "download.bin"
        } else {
            raw_filename
        };
        let lower_fn = filename.to_lowercase();

        let (kind, kind_label, folder_sub) = if lower_fn.ends_with(".zip")
            || lower_fn.ends_with(".tar.gz")
            || lower_fn.ends_with(".7z")
            || lower_fn.ends_with(".rar")
            || lower_fn.ends_with(".tar")
        {
            (ContentKind::Archive, "Compressed Archive", "Archives")
        } else if lower_fn.ends_with(".mp4")
            || lower_fn.ends_with(".mkv")
            || lower_fn.ends_with(".mov")
            || lower_fn.ends_with(".webm")
            || lower_fn.ends_with(".avi")
        {
            (ContentKind::Video, "Video Media", "Videos")
        } else if lower_fn.ends_with(".mp3")
            || lower_fn.ends_with(".wav")
            || lower_fn.ends_with(".flac")
            || lower_fn.ends_with(".aac")
            || lower_fn.ends_with(".m4a")
        {
            (ContentKind::Audio, "Audio Track", "Audio")
        } else if lower_fn.ends_with(".pdf")
            || lower_fn.ends_with(".docx")
            || lower_fn.ends_with(".csv")
            || lower_fn.ends_with(".txt")
        {
            (ContentKind::Document, "Document", "Documents")
        } else if lower_fn.ends_with(".gguf")
            || lower_fn.ends_with(".safetensors")
            || lower_fn.ends_with(".onnx")
            || lower_fn.ends_with(".pt")
        {
            (ContentKind::AiModel, "AI Model Checkpoint", "Models")
        } else if lower_fn.ends_with(".parquet")
            || lower_fn.ends_with(".arrow")
            || lower_fn.ends_with(".jsonl")
        {
            (ContentKind::AiDataset, "AI Dataset Shard", "Datasets")
        } else if lower_fn.ends_with(".png")
            || lower_fn.ends_with(".jpg")
            || lower_fn.ends_with(".jpeg")
            || lower_fn.ends_with(".webp")
            || lower_fn.ends_with(".gif")
        {
            (ContentKind::Image, "Image Asset", "Images")
        } else if lower_fn.ends_with(".exe")
            || lower_fn.ends_with(".msi")
            || lower_fn.ends_with(".iso")
        {
            (ContentKind::Software, "Software Installer", "Software")
        } else {
            (
                ContentKind::GenericFile,
                "Direct File Download",
                "Downloads",
            )
        };

        DetectionResult {
            raw_url: url_str.to_string(),
            is_valid: true,
            platform: PlatformKind::DirectFile,
            platform_name: host.clone(),
            platform_icon: "download".to_string(),
            content_kind: kind,
            content_type_label: kind_label.to_string(),
            title: filename.to_string(),
            suggested_filename: filename.to_string(),
            expected_size_bytes: None,
            suggested_folder: format!("{}\\{}", default_download_dir, folder_sub),
            requires_auth: false,
            auth_note: None,
            client_handoff_protocol: None,
            variants: vec![DownloadVariant {
                label: format!("Standard HTTP Range Download ({})", filename),
                quality: None,
                format: filename.rsplit('.').next().unwrap_or("bin").to_string(),
                approx_size_bytes: None,
                download_url: url_str.to_string(),
            }],
            error_message: None,
        }
    }
}
