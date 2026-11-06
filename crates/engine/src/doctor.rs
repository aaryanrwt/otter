use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub timestamp: String,
    pub otter_version: String,
    pub os_info: String,
    pub checks: Vec<DoctorCheckItem>,
    pub overall_status: DoctorStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DoctorStatus {
    Pass,
    Warning,
    Fail,
    NotTested,
}

impl std::fmt::Display for DoctorStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DoctorStatus::Pass => write!(f, "PASS"),
            DoctorStatus::Warning => write!(f, "WARN"),
            DoctorStatus::Fail => write!(f, "FAIL"),
            DoctorStatus::NotTested => write!(f, "N/A"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheckItem {
    pub category: String,
    pub name: String,
    pub status: DoctorStatus,
    pub message: String,
    pub latency_ms: Option<u64>,
}

pub struct DoctorEngine {
    client: reqwest::Client,
}

impl DoctorEngine {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .user_agent("otter-doctor/0.1.0")
            .build()?;
        Ok(Self { client })
    }

    pub async fn run_diagnostics(&self, target_dir: Option<PathBuf>, full: bool) -> DoctorReport {
        let mut checks = Vec::new();

        // 1. Environment Checks
        checks.push(DoctorCheckItem {
            category: "Environment".to_string(),
            name: "OTTER Version".to_string(),
            status: DoctorStatus::Pass,
            message: format!(
                "v{} (Rust 2026 toolchain / 2024 Edition, Release profile)",
                env!("CARGO_PKG_VERSION")
            ),
            latency_ms: None,
        });

        let os_str = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
        checks.push(DoctorCheckItem {
            category: "Environment".to_string(),
            name: "Operating System".to_string(),
            status: DoctorStatus::Pass,
            message: format!("{} on {}", os_str, whoami_hostname()),
            latency_ms: None,
        });

        // 2. Storage Checks
        let target = target_dir.unwrap_or_else(|| {
            if Path::new("D:\\").exists() {
                PathBuf::from("D:\\")
            } else {
                PathBuf::from(".")
            }
        });

        let target_str = target.display().to_string();
        if let Some(space) = crate::disk::get_available_disk_space(&target) {
            let gib = space as f64 / 1_073_741_824.0;
            let status = if gib > 20.0 {
                DoctorStatus::Pass
            } else if gib > 5.0 {
                DoctorStatus::Warning
            } else {
                DoctorStatus::Fail
            };
            checks.push(DoctorCheckItem {
                category: "Storage".to_string(),
                name: format!("Disk Free Space ({})", target_str),
                status,
                message: format!("{:.2} GiB available", gib),
                latency_ms: None,
            });
        } else {
            checks.push(DoctorCheckItem {
                category: "Storage".to_string(),
                name: format!("Disk Free Space ({})", target_str),
                status: DoctorStatus::Warning,
                message: "Could not query available disk space via Win32 API".to_string(),
                latency_ms: None,
            });
        }

        // Test Write Permission
        let test_file = target.join(".otter_doctor_probe.tmp");
        match std::fs::write(&test_file, b"otter_probe") {
            Ok(_) => {
                let _ = std::fs::remove_file(&test_file);
                checks.push(DoctorCheckItem {
                    category: "Storage".to_string(),
                    name: "Disk Write Permissions".to_string(),
                    status: DoctorStatus::Pass,
                    message: "Writable and temporary files safely removable".to_string(),
                    latency_ms: None,
                });
            }
            Err(e) => {
                checks.push(DoctorCheckItem {
                    category: "Storage".to_string(),
                    name: "Disk Write Permissions".to_string(),
                    status: DoctorStatus::Fail,
                    message: format!("Write permission denied: {}", e),
                    latency_ms: None,
                });
            }
        }

        // 3. Network DNS & TLS
        let dns_start = Instant::now();
        match tokio::net::lookup_host("huggingface.co:443").await {
            Ok(mut addrs) => {
                let first = addrs.next().map(|a| a.to_string()).unwrap_or_default();
                checks.push(DoctorCheckItem {
                    category: "Network".to_string(),
                    name: "DNS Resolution (huggingface.co)".to_string(),
                    status: DoctorStatus::Pass,
                    message: format!("Resolved to {}", first),
                    latency_ms: Some(dns_start.elapsed().as_millis() as u64),
                });
            }
            Err(e) => {
                checks.push(DoctorCheckItem {
                    category: "Network".to_string(),
                    name: "DNS Resolution (huggingface.co)".to_string(),
                    status: DoctorStatus::Fail,
                    message: format!("DNS lookup failed: {}", e),
                    latency_ms: Some(dns_start.elapsed().as_millis() as u64),
                });
            }
        }

        // 4. Registry Reachability: Hugging Face
        let hf_start = Instant::now();
        match self
            .client
            .get("https://huggingface.co/api/models?limit=1")
            .send()
            .await
        {
            Ok(resp) => {
                let status_code = resp.status();
                let lat = hf_start.elapsed().as_millis() as u64;
                if status_code.is_success() {
                    checks.push(DoctorCheckItem {
                        category: "Registries".to_string(),
                        name: "Hugging Face Model API".to_string(),
                        status: DoctorStatus::Pass,
                        message: format!("HTTP {} OK", status_code),
                        latency_ms: Some(lat),
                    });
                } else {
                    checks.push(DoctorCheckItem {
                        category: "Registries".to_string(),
                        name: "Hugging Face Model API".to_string(),
                        status: DoctorStatus::Warning,
                        message: format!("HTTP {}", status_code),
                        latency_ms: Some(lat),
                    });
                }
            }
            Err(e) => {
                checks.push(DoctorCheckItem {
                    category: "Registries".to_string(),
                    name: "Hugging Face Model API".to_string(),
                    status: DoctorStatus::Fail,
                    message: format!("Request failed: {}", e),
                    latency_ms: Some(hf_start.elapsed().as_millis() as u64),
                });
            }
        }

        // 5. Registry Reachability: Ollama Registry
        let ol_start = Instant::now();
        match self
            .client
            .get("https://registry.ollama.ai/v2/")
            .send()
            .await
        {
            Ok(resp) => {
                let status_code = resp.status();
                let lat = ol_start.elapsed().as_millis() as u64;
                if status_code.is_success() || status_code == reqwest::StatusCode::UNAUTHORIZED {
                    // v2 root returns 401 Unauthorized or 200 by design for Docker registry spec
                    checks.push(DoctorCheckItem {
                        category: "Registries".to_string(),
                        name: "Ollama v2 Registry CDN".to_string(),
                        status: DoctorStatus::Pass,
                        message: format!("Endpoint responsive (HTTP {})", status_code),
                        latency_ms: Some(lat),
                    });
                } else {
                    checks.push(DoctorCheckItem {
                        category: "Registries".to_string(),
                        name: "Ollama v2 Registry CDN".to_string(),
                        status: DoctorStatus::Warning,
                        message: format!("HTTP {}", status_code),
                        latency_ms: Some(lat),
                    });
                }
            }
            Err(e) => {
                checks.push(DoctorCheckItem {
                    category: "Registries".to_string(),
                    name: "Ollama v2 Registry CDN".to_string(),
                    status: DoctorStatus::Fail,
                    message: format!("Connection failed: {}", e),
                    latency_ms: Some(ol_start.elapsed().as_millis() as u64),
                });
            }
        }

        // 6. HTTP Range & Resume Probe
        if full {
            let probe_start = Instant::now();
            let test_url = "https://huggingface.co/datasets/HuggingFaceFW/fineweb/resolve/main/sample/10BT/014_00000.parquet";
            match crate::probe::probe_url(&self.client, test_url).await {
                Ok(probe_info) => {
                    let lat = probe_start.elapsed().as_millis() as u64;
                    if probe_info.resumable {
                        checks.push(DoctorCheckItem {
                            category: "Capabilities".to_string(),
                            name: "HTTP 206 Range & Resumption".to_string(),
                            status: DoctorStatus::Pass,
                            message: format!(
                                "Supported (Size: {}, ETag: {})",
                                probe_info
                                    .total_size
                                    .map(|s| format!("{} B", s))
                                    .unwrap_or_else(|| "Unknown".into()),
                                probe_info.etag.unwrap_or_else(|| "None".into())
                            ),
                            latency_ms: Some(lat),
                        });
                    } else {
                        checks.push(DoctorCheckItem {
                            category: "Capabilities".to_string(),
                            name: "HTTP 206 Range & Resumption".to_string(),
                            status: DoctorStatus::Warning,
                            message: "Server did not accept Range request".to_string(),
                            latency_ms: Some(lat),
                        });
                    }
                }
                Err(e) => {
                    checks.push(DoctorCheckItem {
                        category: "Capabilities".to_string(),
                        name: "HTTP 206 Range & Resumption".to_string(),
                        status: DoctorStatus::Warning,
                        message: format!("Probe failed: {}", e),
                        latency_ms: Some(probe_start.elapsed().as_millis() as u64),
                    });
                }
            }
        }

        let overall_status = if checks.iter().any(|c| c.status == DoctorStatus::Fail) {
            DoctorStatus::Fail
        } else if checks.iter().any(|c| c.status == DoctorStatus::Warning) {
            DoctorStatus::Warning
        } else {
            DoctorStatus::Pass
        };

        DoctorReport {
            timestamp: chrono::Utc::now().to_rfc3339(),
            otter_version: env!("CARGO_PKG_VERSION").to_string(),
            os_info: os_str,
            checks,
            overall_status,
        }
    }
}

fn whoami_hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".to_string())
}
