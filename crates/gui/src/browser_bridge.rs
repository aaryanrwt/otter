use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserDownloadItem {
    pub id: i64,
    pub filename: String,
    pub url: String,
    pub sanitized_url: String,
    pub domain: String,
    pub target_path: String,
    pub mime_type: String,
    pub total_bytes: Option<u64>,
    pub received_bytes: u64,
    pub state: String,         // "in_progress", "paused", "complete", "interrupted"
    pub status: String, // "Detected", "Eligible for OTTER", "OTTER Engine Active", "Paused in Chrome", etc.
    pub engine_status: String, // "Native Browser Download" or "OTTER Engine Active"
    pub is_eligible: bool,
    pub is_otter_managed: bool,
    pub current_speed_bps: f64,
    pub average_speed_bps: f64,
    pub start_time_ms: u64,
    pub last_update_ms: u64,
    #[serde(skip)]
    pub last_bytes: u64,
    #[serde(skip)]
    pub last_instant: Option<Instant>,
    pub error: Option<String>,
    pub otter_job_id: Option<String>,
}

#[derive(Clone)]
pub struct BrowserBridge {
    pub items: Arc<Mutex<HashMap<i64, BrowserDownloadItem>>>,
    pub client: reqwest::Client,
    pub manifest: Arc<tokio::sync::RwLock<Option<Arc<engine::manifest::ManifestStore>>>>,
    pub manager: Arc<tokio::sync::RwLock<Option<Arc<engine::orchestrator::DownloadManager>>>>,
}

impl BrowserBridge {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 Otter/0.1.0")
            .build()
            .unwrap_or_default();

        Self {
            items: Arc::new(Mutex::new(HashMap::new())),
            client,
            manifest: Arc::new(tokio::sync::RwLock::new(None)),
            manager: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    pub async fn set_manifest(&self, manifest: Arc<engine::manifest::ManifestStore>) {
        let mut w = self.manifest.write().await;
        *w = Some(manifest);
    }

    pub async fn set_manager(&self, manager: Arc<engine::orchestrator::DownloadManager>) {
        let mut w = self.manager.write().await;
        *w = Some(manager);
    }

    pub async fn get_all_items(&self) -> Vec<BrowserDownloadItem> {
        let manifest_opt = self.manifest.read().await.clone();
        let mut guard = self.items.lock().await;
        if let Some(manifest) = manifest_opt {
            for item in guard.values_mut() {
                if let Some(ref jid) = item.otter_job_id {
                    if let Ok(Some(job)) = manifest.get_job(jid) {
                        let now = Instant::now();
                        let rx = job.downloaded_bytes;
                        if let Some(last_inst) = item.last_instant {
                            let elapsed = last_inst.elapsed().as_secs_f64();
                            if elapsed >= 0.5 {
                                let delta = rx.saturating_sub(item.last_bytes) as f64;
                                item.current_speed_bps = delta / elapsed;
                                item.last_bytes = rx;
                                item.last_instant = Some(now);
                            }
                        } else {
                            item.last_instant = Some(now);
                            item.last_bytes = rx;
                        }
                        item.received_bytes = rx;
                        if let Some(tot) = job.total_size {
                            item.total_bytes = Some(tot);
                        }
                        match job.status {
                            types::DownloadStatus::Completed => {
                                item.state = "complete".to_string();
                                item.status = "Completed".to_string();
                                item.current_speed_bps = 0.0;
                            }
                            types::DownloadStatus::Error => {
                                item.state = "interrupted".to_string();
                                item.status = "Failed".to_string();
                                item.error = job.error.clone();
                                item.current_speed_bps = 0.0;
                            }
                            types::DownloadStatus::Active | types::DownloadStatus::Queued => {
                                item.state = "in_progress".to_string();
                                item.status = "OTTER Multi-Stream Active (16 Workers)".to_string();
                                item.engine_status = "OTTER Multi-Stream Active (16 Workers)".to_string();
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        guard.values().cloned().collect()
    }

    pub async fn get_item(&self, id: i64) -> Option<BrowserDownloadItem> {
        let guard = self.items.lock().await;
        guard.get(&id).cloned()
    }

    /// Redacts query tokens, signatures, and authentication secrets from the URL
    pub fn sanitize_url(raw: &str) -> (String, String) {
        if let Ok(mut u) = url::Url::parse(raw) {
            let domain = u.host_str().unwrap_or("direct").to_string();
            let mut redacted_pairs = Vec::new();
            for (k, v) in u.query_pairs() {
                let lower_k = k.to_lowercase();
                if lower_k.contains("sig")
                    || lower_k.contains("token")
                    || lower_k.contains("nonce")
                    || lower_k.contains("auth")
                    || lower_k.contains("key")
                    || lower_k.contains("expire")
                {
                    redacted_pairs.push((k.to_string(), "[REDACTED]".to_string()));
                } else {
                    redacted_pairs.push((k.to_string(), v.to_string()));
                }
            }
            u.set_query(None);
            for (k, v) in redacted_pairs {
                u.query_pairs_mut().append_pair(&k, &v);
            }
            (u.to_string(), domain)
        } else {
            (raw.to_string(), "direct".to_string())
        }
    }

    /// Probes the server using Range: bytes=0-0 to verify multi-stream capability
    pub async fn probe_range_support(&self, url: &str) -> (bool, Option<u64>, Option<String>) {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return (false, None, None);
        }

        match self
            .client
            .get(url)
            .header("Range", "bytes=0-0")
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();
                if status == reqwest::StatusCode::PARTIAL_CONTENT {
                    // 206 Partial Content: Server supports HTTP byte ranges!
                    let total_len = resp
                        .headers()
                        .get("Content-Range")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.rsplit('/').next())
                        .and_then(|s| s.parse::<u64>().ok());

                    let etag = resp
                        .headers()
                        .get("ETag")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string());

                    (true, total_len, etag)
                } else if status == reqwest::StatusCode::OK {
                    // 200 OK: Accept-Ranges header might still be present
                    let accept_ranges = resp
                        .headers()
                        .get("Accept-Ranges")
                        .and_then(|v| v.to_str().ok())
                        .map(|v| v.to_lowercase().contains("bytes"))
                        .unwrap_or(false);

                    let cl = resp.content_length();
                    let etag = resp
                        .headers()
                        .get("ETag")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string());

                    (accept_ranges, cl, etag)
                } else {
                    (false, None, None)
                }
            }
            Err(_) => (false, None, None),
        }
    }

    pub async fn handle_download_created(
        &self,
        id: i64,
        raw_url: String,
        final_url: Option<String>,
        filename: String,
        target_path: String,
        mime_type: String,
        total_bytes: Option<u64>,
        received_bytes: u64,
        auto_handoff: bool,
    ) -> String {
        let (sanitized_url, domain) = Self::sanitize_url(&raw_url);

        // Probe for Range acceleration eligibility (prefer resolved final CDN url if available)
        let probe_target = final_url.as_deref().unwrap_or(&raw_url);
        let mut probe_res = self.probe_range_support(probe_target).await;
        let mut active_download_url = probe_target.to_string();

        if !probe_res.0 && probe_target != raw_url.as_str() {
            probe_res = self.probe_range_support(&raw_url).await;
            if probe_res.0 {
                active_download_url = raw_url.clone();
            }
        }

        let is_eligible = probe_res.0;
        let probed_size = probe_res.1;
        let final_total = total_bytes.or(probed_size);

        let initial_status = if is_eligible {
            "Eligible for OTTER".to_string()
        } else {
            "Native Browser Download".to_string()
        };

        let engine_status = if auto_handoff && is_eligible {
            "OTTER Engine Active".to_string()
        } else {
            "Native Browser Download".to_string()
        };

        let mut item = BrowserDownloadItem {
            id,
            filename: if filename.is_empty() {
                "download".to_string()
            } else {
                filename
            },
            url: active_download_url.clone(),
            sanitized_url,
            domain,
            target_path,
            mime_type,
            total_bytes: final_total,
            received_bytes,
            state: "in_progress".to_string(),
            status: initial_status,
            engine_status,
            is_eligible,
            is_otter_managed: auto_handoff && is_eligible,
            current_speed_bps: 0.0,
            average_speed_bps: 0.0,
            start_time_ms: 0,
            last_update_ms: 0,
            last_bytes: received_bytes,
            last_instant: Some(Instant::now()),
            error: None,
            otter_job_id: None,
        };

        let action = if auto_handoff {
            let manifest_opt = self.manifest.read().await.clone();
            let manager_opt = self.manager.read().await.clone();
            if let (Some(manifest), Some(manager)) = (manifest_opt, manager_opt) {
                let default_dir = "D:\\".to_string();
                let resolved_dir = BrowserBridge::resolve_download_destination_dir(
                    &item.target_path,
                    &default_dir,
                );
                let _ = std::fs::create_dir_all(&resolved_dir);
                let job_id = format!(
                    "{:x}",
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
                );
                let target_file = resolved_dir.join(&item.filename);

                let job = types::JobRecord {
                    id: job_id.clone(),
                    url: item.url.clone(),
                    filename: item.filename.clone(),
                    destination: target_file.display().to_string(),
                    total_size: item.total_bytes,
                    downloaded_bytes: 0,
                    status: types::DownloadStatus::Queued,
                    resumable: true,
                    etag: None,
                    last_modified: None,
                    expected_sha256: None,
                    computed_sha256: None,
                    error: None,
                    created_at: chrono::Utc::now().timestamp_millis(),
                    completed_at: None,
                };
                let _ = manifest.upsert_job(&job);

                let mgr_clone = manager.clone();
                let jid_clone = job_id.clone();
                tokio::spawn(async move {
                    let token = tokio_util::sync::CancellationToken::new();
                    let _ = mgr_clone.run_job(&jid_clone, 16, token).await;
                });

                item.is_otter_managed = true;
                item.engine_status = "OTTER Multi-Stream Active (16 Workers)".to_string();
                item.otter_job_id = Some(job_id);
                "PAUSE_AND_TAKEOVER".to_string()
            } else {
                "MONITOR_ONLY".to_string()
            }
        } else {
            "MONITOR_ONLY".to_string()
        };

        let mut guard = self.items.lock().await;
        guard.insert(id, item);

        action
    }

    pub async fn handle_download_changed(
        &self,
        id: i64,
        state: Option<String>,
        paused: Option<bool>,
        error: Option<String>,
        received_bytes: Option<u64>,
        total_bytes: Option<u64>,
        filename: Option<String>,
    ) {
        let mut guard = self.items.lock().await;
        if let Some(item) = guard.get_mut(&id) {
            let now = Instant::now();
            if let Some(rx) = received_bytes {
                if let Some(last_inst) = item.last_instant {
                    let elapsed = last_inst.elapsed().as_secs_f64();
                    if elapsed >= 0.5 {
                        let delta = rx.saturating_sub(item.last_bytes) as f64;
                        item.current_speed_bps = delta / elapsed;
                        item.last_bytes = rx;
                        item.last_instant = Some(now);
                    }
                } else {
                    item.last_instant = Some(now);
                    item.last_bytes = rx;
                }
                item.received_bytes = rx;
            }

            if let Some(tot) = total_bytes {
                if tot > 0 {
                    item.total_bytes = Some(tot);
                }
            }

            if let Some(fname) = filename {
                if !fname.is_empty() && fname != "download" {
                    item.filename = fname;
                }
            }

            if let Some(st) = state {
                item.state = st.clone();
                if st == "complete" {
                    item.status = "Completed".to_string();
                    item.current_speed_bps = 0.0;
                } else if st == "interrupted" {
                    item.status = "Failed".to_string();
                    item.error = error.clone();
                    item.current_speed_bps = 0.0;
                }
            }

            if item.otter_job_id.is_none() && item.state != "complete" {
                let manifest_opt = self.manifest.read().await.clone();
                let manager_opt = self.manager.read().await.clone();
                if let (Some(manifest), Some(manager)) = (manifest_opt, manager_opt) {
                    let default_dir = "D:\\".to_string();
                    let resolved_dir = BrowserBridge::resolve_download_destination_dir(
                        &item.target_path,
                        &default_dir,
                    );
                    let _ = std::fs::create_dir_all(&resolved_dir);
                    let job_id = format!(
                        "{:x}",
                        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
                    );
                    let target_file = resolved_dir.join(&item.filename);

                    let job = types::JobRecord {
                        id: job_id.clone(),
                        url: item.url.clone(),
                        filename: item.filename.clone(),
                        destination: target_file.display().to_string(),
                        total_size: item.total_bytes,
                        downloaded_bytes: item.received_bytes,
                        status: types::DownloadStatus::Queued,
                        resumable: true,
                        etag: None,
                        last_modified: None,
                        expected_sha256: None,
                        computed_sha256: None,
                        error: None,
                        created_at: chrono::Utc::now().timestamp_millis(),
                        completed_at: None,
                    };
                    let _ = manifest.upsert_job(&job);

                    let mgr_clone = manager.clone();
                    let jid_clone = job_id.clone();
                    tokio::spawn(async move {
                        let token = tokio_util::sync::CancellationToken::new();
                        let _ = mgr_clone.run_job(&jid_clone, 16, token).await;
                    });

                    item.is_otter_managed = true;
                    item.engine_status = "OTTER Multi-Stream Active (16 Workers)".to_string();
                    item.status = "OTTER Multi-Stream Active (16 Workers)".to_string();
                    item.otter_job_id = Some(job_id);
                }
            }

            if let Some(p) = paused {
                if p && item.state != "complete" {
                    item.status = "Paused in Chrome".to_string();
                    item.current_speed_bps = 0.0;
                }
            }
        }
    }

    pub async fn set_item_handed_off(&self, id: i64, job_id: String) {
        let mut guard = self.items.lock().await;
        if let Some(item) = guard.get_mut(&id) {
            item.is_otter_managed = true;
            item.status = "OTTER Engine Active".to_string();
            item.engine_status = "OTTER Engine Active".to_string();
            item.otter_job_id = Some(job_id);
        }
    }

    /// Dynamically discovers Chrome's configured download directories across all active Chrome profiles
    /// by inspecting `%LOCALAPPDATA%\Google\Chrome\User Data\*\Preferences` (`download.default_directory`),
    /// falling back to the Windows user Downloads folder (`%USERPROFILE%\Downloads`) and OTTER's configured directory.
    /// Never uses hardcoded drive or user paths.
    pub fn discover_chrome_download_dirs(
        otter_default_dir: Option<&str>,
    ) -> Vec<std::path::PathBuf> {
        let mut dirs: Vec<std::path::PathBuf> = Vec::new();
        let mut push_unique = |p: std::path::PathBuf| {
            if p.as_os_str().is_empty() {
                return;
            }
            let norm = p.to_string_lossy().to_lowercase();
            if !dirs
                .iter()
                .any(|existing| existing.to_string_lossy().to_lowercase() == norm)
            {
                dirs.push(p);
            }
        };

        // 1. Inspect Chrome User Data profiles for `download.default_directory`
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let chrome_ud = std::path::PathBuf::from(local_app_data)
                .join("Google")
                .join("Chrome")
                .join("User Data");
            if let Ok(entries) = std::fs::read_dir(&chrome_ud) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name == "Default" || name.starts_with("Profile ") {
                        let pref_path = entry.path().join("Preferences");
                        if let Ok(raw) = std::fs::read_to_string(&pref_path) {
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
                                if let Some(dl_dir) = json
                                    .get("download")
                                    .and_then(|d| d.get("default_directory"))
                                    .and_then(|v| v.as_str())
                                {
                                    let p = std::path::PathBuf::from(dl_dir);
                                    if p.exists() {
                                        push_unique(p);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. Include standard OS user Downloads directory via USERPROFILE
        if let Ok(user_profile) = std::env::var("USERPROFILE") {
            let user_dl = std::path::PathBuf::from(user_profile).join("Downloads");
            if user_dl.exists() {
                push_unique(user_dl);
            }
        }

        // 3. Include OTTER's configured download directory if provided
        if let Some(otter_dir) = otter_default_dir {
            if !otter_dir.trim().is_empty() {
                push_unique(std::path::PathBuf::from(otter_dir));
            }
        }

        dirs
    }

    /// Resolves the destination directory for a browser download handoff without hardcoding paths.
    /// Preserves the parent directory of Chrome's `target_path` (including custom Save As folders),
    /// or falls back to Chrome's configured default download directory, or OTTER's configured directory.
    pub fn resolve_download_destination_dir(
        target_path: &str,
        otter_default_dir: &str,
    ) -> std::path::PathBuf {
        if !target_path.trim().is_empty() {
            let p = std::path::Path::new(target_path);
            if p.is_dir() {
                return p.to_path_buf();
            }
            if let Some(parent) = p.parent() {
                if !parent.as_os_str().is_empty() {
                    return parent.to_path_buf();
                }
            }
        }
        let discovered = Self::discover_chrome_download_dirs(Some(otter_default_dir));
        if let Some(first) = discovered.into_iter().next() {
            return first;
        }
        std::path::PathBuf::from(otter_default_dir)
    }
}

pub struct BrowserBridgeServer {
    bridge: BrowserBridge,
    port: u16,
}

impl BrowserBridgeServer {
    pub fn new(bridge: BrowserBridge, port: u16) -> Self {
        Self { bridge, port }
    }

    pub async fn run(self) -> Result<()> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        let listener = match TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!(
                    "[BrowserBridgeServer] Could not bind on 127.0.0.1:{}: {}",
                    self.port, e
                );
                return Err(e.into());
            }
        };

        println!(
            "[BrowserBridgeServer] Listening for Chrome extension events on http://127.0.0.1:{}",
            self.port
        );

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let bridge_clone = self.bridge.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, bridge_clone).await {
                            eprintln!("[BrowserBridgeServer] Connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("[BrowserBridgeServer] Accept error: {}", e);
                }
            }
        }
    }
}

async fn handle_connection(mut stream: TcpStream, bridge: BrowserBridge) -> Result<()> {
    let mut buffer = [0u8; 8192];
    let n = stream.read(&mut buffer).await?;
    if n == 0 {
        return Ok(());
    }

    let req_str = String::from_utf8_lossy(&buffer[..n]);
    let mut lines = req_str.lines();
    let first_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();

    if parts.len() < 2 {
        return Ok(());
    }

    let method = parts[0];
    let path = parts[1];

    // CORS preflight
    if method == "OPTIONS" {
        let resp = "HTTP/1.1 200 OK\r\n\
                    Access-Control-Allow-Origin: *\r\n\
                    Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
                    Access-Control-Allow-Headers: Content-Type\r\n\
                    Content-Length: 0\r\n\
                    Connection: close\r\n\r\n";
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    // Find JSON body if POST
    let body = if let Some(idx) = req_str.find("\r\n\r\n") {
        &req_str[idx + 4..]
    } else {
        ""
    };

    match (method, path) {
        ("GET", "/api/browser/status") => {
            let resp_json = serde_json::json!({
                "status": "online",
                "engine": "OTTER Download Engine",
                "version": "0.1.0",
                "handshake": "ok"
            });
            send_json_response(&mut stream, 200, &resp_json).await?;
        }
        ("GET", "/api/browser/downloads") => {
            let items = bridge.get_all_items().await;
            let resp_json = serde_json::json!({
                "downloads": items,
                "count": items.len()
            });
            send_json_response(&mut stream, 200, &resp_json).await?;
        }
        ("GET", "/api/browser/history") => {
            let manifest_opt = bridge.manifest.read().await.clone();
            let mut history_items = Vec::new();
            if let Some(m) = manifest_opt {
                if let Ok(jobs) = m.list_jobs() {
                    for j in jobs
                        .iter()
                        .filter(|j| j.status == types::DownloadStatus::Completed)
                    {
                        let total_b = j.total_size.unwrap_or(j.downloaded_bytes);
                        let elapsed_desc = if let Some(comp) = j.completed_at {
                            let now = chrono::Utc::now().timestamp_millis();
                            let diff_sec = (now.saturating_sub(comp) / 1000) as u64;
                            if diff_sec < 60 {
                                "Just now".to_string()
                            } else if diff_sec < 3600 {
                                format!("{} min ago", diff_sec / 60)
                            } else if diff_sec < 86400 {
                                format!("{} hours ago", diff_sec / 3600)
                            } else {
                                "Yesterday".to_string()
                            }
                        } else {
                            "Completed".to_string()
                        };

                        history_items.push(serde_json::json!({
                            "id": j.id,
                            "filename": j.filename,
                            "url": j.url,
                            "destination": j.destination,
                            "total_bytes": total_b,
                            "total_str": format_bytes_simple(total_b),
                            "completed_at": j.completed_at,
                            "elapsed_desc": elapsed_desc,
                            "status": "Completed"
                        }));
                    }
                }
            }
            let resp_json = serde_json::json!({
                "history": history_items,
                "count": history_items.len()
            });
            send_json_response(&mut stream, 200, &resp_json).await?;
        }
        ("POST", "/api/browser/history/clear") => {
            let manifest_opt = bridge.manifest.read().await.clone();
            if let Some(m) = manifest_opt {
                if let Ok(jobs) = m.list_jobs() {
                    for j in jobs
                        .iter()
                        .filter(|j| j.status == types::DownloadStatus::Completed)
                    {
                        let _ = m.delete_job(&j.id);
                    }
                }
            }
            let resp_json = serde_json::json!({ "success": true });
            send_json_response(&mut stream, 200, &resp_json).await?;
        }
        ("POST", "/api/browser/download/created") => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
                let id = v.get("id").and_then(|x| x.as_i64()).unwrap_or(0);
                let url = v
                    .get("url")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let filename = v
                    .get("filename")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let target_path = v
                    .get("target_path")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let mime = v
                    .get("mime_type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let total_bytes = v.get("total_bytes").and_then(|x| x.as_u64());
                let rx = v
                    .get("received_bytes")
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0);
                let auto_handoff = v
                    .get("auto_handoff_requested")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);

                let final_url = v
                    .get("final_url")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());

                let handoff_action = bridge
                    .handle_download_created(
                        id,
                        url,
                        final_url,
                        filename,
                        target_path,
                        mime,
                        total_bytes,
                        rx,
                        auto_handoff,
                    )
                    .await;

                let resp_json = serde_json::json!({
                    "success": true,
                    "id": id,
                    "handoff": handoff_action
                });
                send_json_response(&mut stream, 200, &resp_json).await?;
            } else {
                send_error_response(&mut stream, 400, "Invalid JSON body").await?;
            }
        }
        ("POST", "/api/browser/download/changed") => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
                let id = v.get("id").and_then(|x| x.as_i64()).unwrap_or(0);
                let state = v
                    .get("state")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let paused = v.get("paused").and_then(|x| x.as_bool());
                let error = v
                    .get("error")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let total_bytes = v.get("total_bytes").and_then(|x| x.as_u64());
                let filename = v
                    .get("filename")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());

                bridge
                    .handle_download_changed(id, state, paused, error, None, total_bytes, filename)
                    .await;

                let resp_json = serde_json::json!({ "success": true, "id": id });
                send_json_response(&mut stream, 200, &resp_json).await?;
            } else {
                send_error_response(&mut stream, 400, "Invalid JSON body").await?;
            }
        }
        ("POST", "/api/browser/download/handoff") => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
                let id = v.get("id").and_then(|x| x.as_i64()).unwrap_or(0);
                if let Some(item) = bridge.get_item(id).await {
                    let manifest_opt = bridge.manifest.read().await.clone();
                    let manager_opt = bridge.manager.read().await.clone();
                    if let (Some(manifest), Some(manager)) = (manifest_opt, manager_opt) {
                        let default_dir = "D:\\".to_string();
                        let resolved_dir = BrowserBridge::resolve_download_destination_dir(
                            &item.target_path,
                            &default_dir,
                        );
                        let _ = std::fs::create_dir_all(&resolved_dir);
                        let job_id = format!(
                            "{:x}",
                            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
                        );
                        let target_path = resolved_dir.join(&item.filename);

                        let job = types::JobRecord {
                            id: job_id.clone(),
                            url: item.url.clone(),
                            filename: item.filename.clone(),
                            destination: target_path.display().to_string(),
                            total_size: item.total_bytes,
                            downloaded_bytes: 0,
                            status: types::DownloadStatus::Queued,
                            resumable: true,
                            etag: None,
                            last_modified: None,
                            expected_sha256: None,
                            computed_sha256: None,
                            error: None,
                            created_at: chrono::Utc::now().timestamp_millis(),
                            completed_at: None,
                        };
                        let _ = manifest.upsert_job(&job);
                        bridge.set_item_handed_off(id, job_id.clone()).await;

                        let mgr_clone = manager.clone();
                        let jid_clone = job_id.clone();
                        tokio::spawn(async move {
                            let token = tokio_util::sync::CancellationToken::new();
                            let _ = mgr_clone.run_job(&jid_clone, 16, token).await;
                        });

                        let resp_json = serde_json::json!({
                            "success": true,
                            "id": id,
                            "job_id": job_id,
                            "status": "HANDED_OFF"
                        });
                        send_json_response(&mut stream, 200, &resp_json).await?;
                    } else {
                        let resp_json = serde_json::json!({
                            "success": false,
                            "error": "Engine manager or manifest not initialized"
                        });
                        send_json_response(&mut stream, 503, &resp_json).await?;
                    }
                } else {
                    send_error_response(&mut stream, 404, "Download item not found").await?;
                }
            } else {
                send_error_response(&mut stream, 400, "Invalid JSON body").await?;
            }
        }
        ("POST", "/api/browser/open_folder") => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
                let path_str = v.get("path").and_then(|p| p.as_str()).unwrap_or("");
                if !path_str.is_empty() {
                    let p = std::path::PathBuf::from(path_str);
                    if p.exists() {
                        let _ = std::process::Command::new("explorer")
                            .arg(format!("/select,{}", p.display()))
                            .spawn();
                    } else if let Some(parent) = p.parent() {
                        if parent.exists() {
                            let _ = std::process::Command::new("explorer").arg(parent).spawn();
                        }
                    }
                }
                let resp_json = serde_json::json!({ "success": true });
                send_json_response(&mut stream, 200, &resp_json).await?;
            } else {
                send_error_response(&mut stream, 400, "Invalid JSON body").await?;
            }
        }
        _ => {
            send_error_response(&mut stream, 404, "Endpoint not found").await?;
        }
    }

    Ok(())
}

fn format_bytes_simple(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

async fn send_json_response(
    stream: &mut TcpStream,
    status: u16,
    data: &serde_json::Value,
) -> Result<()> {
    let body_str = serde_json::to_string(data)?;
    let resp = format!(
        "HTTP/1.1 {} OK\r\n\
         Content-Type: application/json\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{}",
        status,
        body_str.len(),
        body_str
    );
    stream.write_all(resp.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

async fn send_error_response(stream: &mut TcpStream, status: u16, msg: &str) -> Result<()> {
    let body_str = serde_json::json!({ "error": msg }).to_string();
    let resp = format!(
        "HTTP/1.1 {} Error\r\n\
         Content-Type: application/json\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{}",
        status,
        body_str.len(),
        body_str
    );
    stream.write_all(resp.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_url_sanitization_redacts_tokens() {
        let raw = "https://cdn.example.com/file.zip?sig=secret123&token=abc&nonce=999&normal=val";
        let (sanitized, domain) = BrowserBridge::sanitize_url(raw);
        assert_eq!(domain, "cdn.example.com");
        assert!(sanitized.contains("sig=%5BREDACTED%5D") || sanitized.contains("sig=[REDACTED]"));
        assert!(
            sanitized.contains("token=%5BREDACTED%5D") || sanitized.contains("token=[REDACTED]")
        );
        assert!(sanitized.contains("normal=val"));
    }

    #[tokio::test]
    async fn test_browser_download_created_and_changed() {
        let bridge = BrowserBridge::new();
        let action = bridge
            .handle_download_created(
                101,
                "https://example.com/test.zip".into(),
                None,
                "test.zip".into(),
                "C:\\Downloads\\test.zip".into(),
                "application/zip".into(),
                Some(104857600),
                0,
                false,
            )
            .await;

        assert_eq!(action, "MONITOR_ONLY");

        let items = bridge.get_all_items().await;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].filename, "test.zip");
        assert_eq!(items[0].total_bytes, Some(104857600));

        // State change
        bridge
            .handle_download_changed(
                101,
                Some("complete".into()),
                None,
                None,
                Some(104857600),
                None,
                None,
            )
            .await;

        let updated = bridge.get_item(101).await.unwrap();
        assert_eq!(updated.state, "complete");
        assert_eq!(updated.status, "Completed");
        assert_eq!(updated.received_bytes, 104857600);
    }

    #[test]
    fn test_dynamic_location_discovery_and_resolution() {
        let dirs = BrowserBridge::discover_chrome_download_dirs(None);
        assert!(
            !dirs.is_empty(),
            "Should dynamically discover at least one valid download directory"
        );

        // Should preserve custom Save-As parent directory from target_path
        let custom_target = "E:\\CustomGameFolder\\game_setup.zip";
        let resolved =
            BrowserBridge::resolve_download_destination_dir(custom_target, "C:\\Fallback");
        assert_eq!(resolved, std::path::PathBuf::from("E:\\CustomGameFolder"));
    }
}
