use anyhow::Result;
use engine::manifest::ManifestStore;
use engine::orchestrator::DownloadManager;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use types::ProgressSnapshot;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

fn default_true() -> bool {
    true
}
fn default_16() -> usize {
    16
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiSettings {
    #[serde(default)]
    pub user_name: String,
    pub default_download_dir: String,
    pub concurrent_downloads: usize,
    pub bandwidth_limit_mb_s: Option<f64>,
    pub start_with_windows: bool,
    pub minimize_to_tray: bool,
    pub live_detection: bool,
    pub theme: String,
    pub language: String,
    #[serde(default = "default_true")]
    pub auto_detect_urls: bool,
    #[serde(default = "default_true")]
    pub show_detection_preview: bool,
    #[serde(default = "default_true")]
    pub aimd_adaptive_concurrency: bool,
    #[serde(default = "default_16")]
    pub max_connections_per_host: usize,
    #[serde(default = "default_true")]
    pub auto_verify_sha256: bool,
    #[serde(default = "default_true")]
    pub crash_recovery: bool,
    #[serde(default = "default_true")]
    pub notify_on_completed: bool,
    #[serde(default = "default_true")]
    pub notify_on_failed: bool,
    #[serde(default)]
    pub proxy_url: Option<String>,
}

impl Default for GuiSettings {
    fn default() -> Self {
        let default_dir = if Path::new("D:\\").exists() {
            "D:\\Downloads\\Otter".to_string()
        } else {
            let user_profile = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
            format!("{}\\Downloads\\Otter", user_profile)
        };

        Self {
            user_name: String::new(),
            default_download_dir: default_dir,
            concurrent_downloads: 16,
            bandwidth_limit_mb_s: None,
            start_with_windows: false,
            minimize_to_tray: true,
            live_detection: true,
            theme: "light".to_string(),
            language: "English".to_string(),
            auto_detect_urls: true,
            show_detection_preview: true,
            aimd_adaptive_concurrency: true,
            max_connections_per_host: 64,
            auto_verify_sha256: true,
            crash_recovery: true,
            notify_on_completed: true,
            notify_on_failed: true,
            proxy_url: None,
        }
    }
}

impl GuiSettings {
    pub fn load() -> Self {
        let path = Self::settings_path();
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str::<GuiSettings>(&data) {
                return settings;
            }
        }
        let default_s = Self::default();
        let _ = default_s.save();
        default_s
    }

    /// Atomic write to prevent partial file writes
    pub fn save(&self) -> Result<()> {
        let path = Self::settings_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let temp_path = path.with_extension("tmp");
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(&temp_path, data)?;
        let _ = std::fs::rename(&temp_path, &path);

        // Update Windows autostart registry key if toggled
        let _ = update_windows_autostart(self.start_with_windows);
        Ok(())
    }

    fn settings_path() -> PathBuf {
        let local_app_data = std::env::var("LOCALAPPDATA")
            .unwrap_or_else(|_| "C:\\Users\\Default\\AppData\\Local".to_string());
        PathBuf::from(local_app_data)
            .join("otter")
            .join("gui_settings.json")
    }
}

pub fn update_windows_autostart(enable: bool) -> Result<()> {
    #[cfg(windows)]
    {
        if let Ok(exe_path) = std::env::current_exe() {
            let exe_str = exe_path.to_string_lossy().to_string();
            if enable {
                let _ = std::process::Command::new("reg")
                    .args(&[
                        "add",
                        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                        "/v",
                        "Otter",
                        "/t",
                        "REG_SZ",
                        "/d",
                        &format!("\"{}\"", exe_str),
                        "/f",
                    ])
                    .creation_flags(0x08000000) // CREATE_NO_WINDOW
                    .output();
            } else {
                let _ = std::process::Command::new("reg")
                    .args(&[
                        "delete",
                        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                        "/v",
                        "Otter",
                        "/f",
                    ])
                    .creation_flags(0x08000000) // CREATE_NO_WINDOW
                    .output();
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSpecs {
    pub device_name: String,
    pub cpu_cores: usize,
    pub total_ram_gb: f64,
    pub free_disk_space_gb: f64,
    pub disk_path: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityItem {
    pub source: String,
    pub active_count: usize,
    pub total_size_str: String,
    pub speed_str: String,
    pub status: String,
    pub is_completed: bool,
}

pub struct AppState {
    pub manager: Arc<DownloadManager>,
    pub manifest: Arc<ManifestStore>,
    pub active_snapshots: Arc<Mutex<std::collections::HashMap<String, ProgressSnapshot>>>,
    pub cancellation_tokens:
        Arc<Mutex<std::collections::HashMap<String, tokio_util::sync::CancellationToken>>>,
    pub thumbnails: Arc<Mutex<std::collections::HashMap<String, String>>>,
    pub settings: Arc<Mutex<GuiSettings>>,
}

impl AppState {
    pub fn new() -> Result<Self> {
        let manifest = Arc::new(ManifestStore::open_default()?);
        let manager = Arc::new(DownloadManager::with_max_host_connections(
            manifest.clone(),
            64,
        )?);
        let settings = Arc::new(Mutex::new(GuiSettings::load()));

        Ok(Self {
            manager,
            manifest,
            active_snapshots: Arc::new(Mutex::new(std::collections::HashMap::new())),
            cancellation_tokens: Arc::new(Mutex::new(std::collections::HashMap::new())),
            thumbnails: Arc::new(Mutex::new(std::collections::HashMap::new())),
            settings,
        })
    }

    pub fn get_system_specs(&self, target_dir: &str) -> SystemSpecs {
        let device_name = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "Desktop PC".to_string());

        let cpu_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(8);

        #[cfg(windows)]
        let total_ram_gb = {
            #[allow(non_snake_case)]
            #[repr(C)]
            struct MemoryStatusEx {
                dw_length: u32,
                dw_memory_load: u32,
                ull_total_phys: u64,
                ull_avail_phys: u64,
                ull_total_page_file: u64,
                ull_avail_page_file: u64,
                ull_total_virtual: u64,
                ull_avail_virtual: u64,
                ull_avail_extended_virtual: u64,
            }
            unsafe {
                #[link(name = "kernel32")]
                unsafe extern "system" {
                    fn GlobalMemoryStatusEx(lpBuffer: *mut MemoryStatusEx) -> i32;
                }
                let mut mem = std::mem::zeroed::<MemoryStatusEx>();
                mem.dw_length = std::mem::size_of::<MemoryStatusEx>() as u32;
                if GlobalMemoryStatusEx(&mut mem) != 0 {
                    (mem.ull_total_phys as f64) / (1024.0 * 1024.0 * 1024.0)
                } else {
                    16.0
                }
            }
        };

        #[cfg(not(windows))]
        let total_ram_gb = 16.0;

        let path = Path::new(target_dir);
        let free_bytes = engine::disk::get_available_disk_space(path).unwrap_or(0);
        let free_disk_space_gb = (free_bytes as f64) / (1024.0 * 1024.0 * 1024.0);

        SystemSpecs {
            device_name,
            cpu_cores,
            total_ram_gb: (total_ram_gb * 10.0).round() / 10.0,
            free_disk_space_gb: (free_disk_space_gb * 10.0).round() / 10.0,
            disk_path: target_dir.to_string(),
        }
    }
}
