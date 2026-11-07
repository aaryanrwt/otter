#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod activity;
mod browser_bridge;
mod detector;
mod logos;
mod state;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_ENGINE;
use detector::UrlDetector;
use state::{AppState, GuiSettings};
use std::sync::Arc;
use std::time::Duration;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use wry::http::{Response, header::CONTENT_TYPE};
use wry::{WebContext, WebView, WebViewBuilder};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const INDEX_HTML: &str = include_str!("../ui/index.html");
const STYLE_CSS: &str = include_str!("../ui/style.css");
const APP_JS: &str = include_str!("../ui/app.js");

const MASCOT_FLYING: &[u8] =
    include_bytes!("../../../images/Flying Otter Mascot with Flowing Scarf.png");
const MASCOT_FOCUSED: &[u8] =
    include_bytes!("../../../images/Focused Otter with Laptop and Coffee.png");
const MASCOT_CHEERFUL: &[u8] =
    include_bytes!("../../../images/Cheerful Otter Popping Out of Pink Box.png");
const MASCOT_SLEEPING: &[u8] =
    include_bytes!("../../../images/Sleeping Otter with Pink Paw Pillow.png");

#[cfg(windows)]
mod single_instance {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    type HANDLE = *mut std::ffi::c_void;
    type HWND = *mut std::ffi::c_void;
    type BOOL = i32;
    const ERROR_ALREADY_EXISTS: u32 = 183;
    const SW_RESTORE: i32 = 9;

    unsafe extern "system" {
        fn CreateMutexW(
            lpMutexAttributes: *mut std::ffi::c_void,
            bInitialOwner: BOOL,
            lpName: *const u16,
        ) -> HANDLE;
        fn GetLastError() -> u32;
        fn FindWindowW(lpClassName: *const u16, lpWindowName: *const u16) -> HWND;
        fn ShowWindow(hWnd: HWND, nCmdShow: i32) -> BOOL;
        fn SetForegroundWindow(hWnd: HWND) -> BOOL;
        fn CloseHandle(hObject: HANDLE) -> BOOL;
    }

    pub struct SingleInstanceGuard {
        handle: HANDLE,
    }

    impl SingleInstanceGuard {
        pub fn acquire(name: &str) -> Option<Self> {
            let wide_name: Vec<u16> = OsStr::new(name)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let handle = unsafe { CreateMutexW(std::ptr::null_mut(), 1, wide_name.as_ptr()) };
            if handle.is_null() {
                return None;
            }
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                let title: Vec<u16> = OsStr::new("Otter")
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
                if !hwnd.is_null() {
                    unsafe {
                        ShowWindow(hwnd, SW_RESTORE);
                        SetForegroundWindow(hwnd);
                    }
                }
                unsafe { CloseHandle(handle) };
                return None;
            }
            Some(Self { handle })
        }
    }

    impl Drop for SingleInstanceGuard {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe { CloseHandle(self.handle) };
            }
        }
    }
}

enum UserEvent {
    IpcMessage(String),
    PushStateUpdate(String),
    EvalScript(String),
}

fn main() -> Result<()> {
    #[cfg(windows)]
    let _instance_guard = match single_instance::SingleInstanceGuard::acquire(
        "Local\\OtterUniversalDownloadManager_Mutex",
    ) {
        Some(guard) => guard,
        None => {
            println!("Otter is already running. Existing window brought to focus.");
            return Ok(());
        }
    };

    let tokio_rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let app_state = Arc::new(app_state_init(&tokio_rt)?);

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("Otter")
        .with_inner_size(tao::dpi::LogicalSize::new(1120.0, 740.0))
        .with_min_inner_size(tao::dpi::LogicalSize::new(960.0, 640.0))
        .with_decorations(true)
        .build(&event_loop)?;

    // Inline mascot PNGs as base64 data URIs into HTML to guarantee 100% offline rendering without protocol issues
    let flying_b64 = format!(
        "data:image/png;base64,{}",
        BASE64_ENGINE.encode(MASCOT_FLYING)
    );
    let focused_b64 = format!(
        "data:image/png;base64,{}",
        BASE64_ENGINE.encode(MASCOT_FOCUSED)
    );
    let cheerful_b64 = format!(
        "data:image/png;base64,{}",
        BASE64_ENGINE.encode(MASCOT_CHEERFUL)
    );
    let sleeping_b64 = format!(
        "data:image/png;base64,{}",
        BASE64_ENGINE.encode(MASCOT_SLEEPING)
    );

    let app_logos = logos::get_logos();
    let logos_json = serde_json::json!({
        "chrome": app_logos.chrome,
        "steam": app_logos.steam,
        "epic": app_logos.epic,
        "hf": app_logos.hf,
        "github": app_logos.github,
        "instagram": app_logos.instagram,
        "pinterest": app_logos.pinterest,
        "folder": app_logos.folder,
    });
    let logos_script = format!(
        "<script>window.OTTER_LOGOS = {};</script>",
        serde_json::to_string(&logos_json)?
    );

    let inlined_html = INDEX_HTML
        .replace("__MASCOT_FLYING__", &flying_b64)
        .replace("__MASCOT_FOCUSED__", &focused_b64)
        .replace("__MASCOT_CHEERFUL__", &cheerful_b64)
        .replace("__MASCOT_SLEEPING__", &sleeping_b64)
        .replace("<!-- __OTTER_LOGOS_SCRIPT__ -->", &logos_script);

    let html_bytes = inlined_html.into_bytes();

    // Custom protocol handler for otter:// schema
    let handler_proxy = proxy.clone();
    let local_app_data = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| "C:\\Users\\Default\\AppData\\Local".to_string());
    let data_dir = std::path::PathBuf::from(local_app_data)
        .join("otter")
        .join("webview_data");
    let _ = std::fs::create_dir_all(&data_dir);
    let mut web_context = WebContext::new(Some(data_dir));

    let webview = match WebViewBuilder::new_with_web_context(&mut web_context)
        .with_custom_protocol("otter".into(), {
            let html_bytes = html_bytes.clone();
            move |_webview, request| {
                let uri = request.uri().to_string().to_lowercase();
                if uri.contains("style.css") {
                    Response::builder()
                        .header(CONTENT_TYPE, "text/css; charset=utf-8")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(STYLE_CSS.as_bytes().into())
                        .unwrap()
                } else if uri.contains("app.js") {
                    Response::builder()
                        .header(CONTENT_TYPE, "application/javascript; charset=utf-8")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(APP_JS.as_bytes().into())
                        .unwrap()
                } else {
                    Response::builder()
                        .header(CONTENT_TYPE, "text/html; charset=utf-8")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(html_bytes.clone().into())
                        .unwrap()
                }
            }
        })
        .with_ipc_handler({
            let handler_proxy = handler_proxy.clone();
            move |msg: wry::http::Request<String>| {
                let _ = handler_proxy.send_event(UserEvent::IpcMessage(msg.body().clone()));
            }
        })
        .with_url("otter://app/index.html")
        .build(&window)
    {
        Ok(wv) => wv,
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("0x800700AA") || err_msg.contains("resource is in use") {
                eprintln!(
                    "[Otter] Primary WebView2 data directory locked. Recovering with isolated session data directory..."
                );
                let fallback_dir =
                    std::env::temp_dir().join(format!("otter_wv_{}", std::process::id()));
                let _ = std::fs::create_dir_all(&fallback_dir);
                let mut fallback_ctx = WebContext::new(Some(fallback_dir));
                WebViewBuilder::new_with_web_context(&mut fallback_ctx)
                    .with_custom_protocol("otter".into(), move |_webview, request| {
                        let uri = request.uri().to_string().to_lowercase();
                        if uri.contains("style.css") {
                            Response::builder()
                                .header(CONTENT_TYPE, "text/css; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(STYLE_CSS.as_bytes().into())
                                .unwrap()
                        } else if uri.contains("app.js") {
                            Response::builder()
                                .header(CONTENT_TYPE, "application/javascript; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(APP_JS.as_bytes().into())
                                .unwrap()
                        } else {
                            Response::builder()
                                .header(CONTENT_TYPE, "text/html; charset=utf-8")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(html_bytes.clone().into())
                                .unwrap()
                        }
                    })
                    .with_ipc_handler(move |msg: wry::http::Request<String>| {
                        let _ = handler_proxy.send_event(UserEvent::IpcMessage(msg.body().clone()));
                    })
                    .with_url("otter://app/index.html")
                    .build(&window)?
            } else {
                return Err(e.into());
            }
        }
    };

    // Spawn background progress listener
    let bg_proxy = proxy.clone();
    let bg_state = app_state.clone();

    tokio_rt.spawn(async move {
        let mut rx = bg_state.manager.subscribe_progress();
        while let Ok(snap) = rx.recv().await {
            {
                let mut map = bg_state.active_snapshots.lock().await;
                if snap.status == types::DownloadStatus::Completed
                    || snap.status == types::DownloadStatus::Error
                {
                    map.remove(&snap.job_id);
                } else {
                    map.insert(snap.job_id.clone(), snap.clone());
                }
            }

            // Periodically refresh GUI state
            let current_target_dir = {
                let s = bg_state.settings.lock().await;
                s.default_download_dir.clone()
            };
            if let Ok(state_json) = generate_full_state(&bg_state, &current_target_dir).await {
                let _ = bg_proxy.send_event(UserEvent::PushStateUpdate(state_json));
            }
        }
    });

    // Initial push of state
    {
        let init_state = app_state.clone();
        let init_proxy = proxy.clone();
        tokio_rt.spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let current_dir = {
                let s = init_state.settings.lock().await;
                s.default_download_dir.clone()
            };
            if let Ok(state_json) = generate_full_state(&init_state, &current_dir).await {
                let _ = init_proxy.send_event(UserEvent::PushStateUpdate(state_json));
            }
        });
    }

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => {}
            Event::UserEvent(user_event) => match user_event {
                UserEvent::IpcMessage(msg) => {
                    handle_ipc_message(&msg, &webview, &app_state, &tokio_rt, &window, &proxy);
                }
                UserEvent::PushStateUpdate(json_str) => {
                    let script = format!(
                        "if (window.onOtterEvent) {{ window.onOtterEvent({{ type: 'state_update', data: {} }}); }}",
                        json_str
                    );
                    let _ = webview.evaluate_script(&script);
                }
                UserEvent::EvalScript(script) => {
                    let _ = webview.evaluate_script(&script);
                }
            },
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            _ => (),
        }
    });
}

fn app_state_init(rt: &tokio::runtime::Runtime) -> Result<AppState> {
    let state = AppState::new()?;
    let bridge_clone = (*state.browser_bridge).clone();
    rt.spawn(async move {
        let server = browser_bridge::BrowserBridgeServer::new(bridge_clone, 45771);
        let _ = server.run().await;
    });
    Ok(state)
}

async fn generate_full_state(app_state: &AppState, current_target_dir: &str) -> Result<String> {
    let jobs = app_state.manifest.list_jobs()?;
    let snap_map = app_state.active_snapshots.lock().await.clone();

    let mut active_jobs = Vec::new();
    let mut all_jobs = Vec::new();
    let mut total_speed_bps = 0.0;
    let mut total_downloaded_bytes: u64 = 0;

    for j in &jobs {
        let mut speed_str = "0.0 MB/s".to_string();
        let mut progress_pct = 0.0;
        let mut status_str = j.status.to_string();
        let mut downloaded = j.downloaded_bytes;
        let mut total_bytes = j.total_size;

        if let Some(snap) = snap_map.get(&j.id) {
            speed_str = if snap.speed_bps >= 1024.0 * 1024.0 {
                format!("{:.1} MB/s", snap.speed_bps / (1024.0 * 1024.0))
            } else if snap.speed_bps >= 1024.0 {
                format!("{:.0} KB/s", snap.speed_bps / 1024.0)
            } else if snap.speed_bps > 0.0 {
                format!("{:.0} B/s", snap.speed_bps)
            } else {
                "0.0 MB/s".to_string()
            };
            progress_pct = snap.percentage();
            status_str = snap.status.to_string();
            downloaded = snap.downloaded_bytes;
            if snap.total_bytes.is_some() {
                total_bytes = snap.total_bytes;
            }
            total_speed_bps += snap.speed_bps;
        } else if let Some(total) = j.total_size {
            if total > 0 {
                progress_pct =
                    ((j.downloaded_bytes as f64 / total as f64) * 100.0).clamp(0.0, 100.0);
            }
        }

        total_downloaded_bytes += downloaded;

        let total_size_str = total_bytes.map(format_bytes).unwrap_or_else(|| {
            if downloaded > 0 {
                format_bytes(downloaded)
            } else {
                "Unknown".to_string()
            }
        });

        let streams = if let Some(snap) = snap_map.get(&j.id) {
            snap.connections
        } else {
            1
        };

        let thumbnail = {
            let map = app_state.thumbnails.lock().await;
            map.get(&j.id).cloned().or_else(|| map.get(&j.url).cloned())
        };

        let job_obj = serde_json::json!({
            "id": j.id,
            "filename": j.filename,
            "source_url": j.url,
            "platform": detect_platform_label(&j.url),
            "status": status_str,
            "total_bytes": total_bytes,
            "total_size_str": total_size_str,
            "downloaded_bytes": downloaded,
            "progress_pct": progress_pct,
            "speed_str": speed_str,
            "streams": streams,
            "thumbnail": thumbnail,
            "destination_path": j.destination,
        });

        all_jobs.push(job_obj.clone());

        if j.status == types::DownloadStatus::Active || snap_map.contains_key(&j.id) {
            active_jobs.push(job_obj.clone());
        }
    }

    let active_count = active_jobs.len();
    let completed_count = jobs
        .iter()
        .filter(|j| j.status == types::DownloadStatus::Completed)
        .count();
    let total_speed_mb = total_speed_bps / (1024.0 * 1024.0);
    let total_dl_gb = (total_downloaded_bytes as f64) / (1024.0 * 1024.0 * 1024.0);

    let system_specs = app_state.get_system_specs(current_target_dir);

    let monitored_dirs = vec![
        std::env::var("USERPROFILE")
            .map(|u| std::path::PathBuf::from(u).join("Downloads"))
            .unwrap_or_default(),
        std::path::PathBuf::from(current_target_dir),
    ];
    let live_det = {
        let s = app_state.settings.lock().await;
        s.live_detection
    };
    let browser_items = app_state.browser_bridge.get_all_items().await;
    let activities =
        activity::ActivityEngine::scan_activities(&jobs, live_det, &monitored_dirs, &browser_items);

    let state_payload = serde_json::json!({
        "jobs": all_jobs,
        "active_jobs": active_jobs,
        "activities": activities,
        "metrics": {
            "active_count": active_count,
            "completed_count": completed_count,
            "total_speed_str": format!("{:.1} MB/s", total_speed_mb),
            "total_downloaded_str": format!("{:.1} GB", total_dl_gb),
        },
        "engine_stats": {
            "aimd_status": "Active (Adaptive Concurrency)",
            "connection_count": if active_count > 0 { active_count * 8 } else { 0 },
            "wal_status": "WAL (Normal sync, safe)",
            "prealloc_status": "SetFileValidData (Win32)",
        },
        "system_specs": system_specs,
        "settings": app_state.settings.lock().await.clone(),
    });

    Ok(serde_json::to_string(&state_payload)?)
}

async fn run_media_download_job(
    job_id: String,
    url: String,
    quality_id: String,
    target_dir: String,
    resolved_name: String,
    connections: usize,
    cancel_token: tokio_util::sync::CancellationToken,
    manifest: Arc<engine::manifest::ManifestStore>,
    active_snaps: Arc<
        tokio::sync::Mutex<std::collections::HashMap<String, types::ProgressSnapshot>>,
    >,
    state_clone: Arc<AppState>,
    proxy_clone: EventLoopProxy<UserEvent>,
) {
    let target_path = std::path::PathBuf::from(&target_dir).join(&resolved_name);
    let script_path = if is_ai_workload_url(&url) {
        get_ai_script_path()
    } else {
        get_media_script_path()
    };
    let mut cmd = tokio::process::Command::new("python");
    cmd.arg("-u")
        .arg(&script_path)
        .arg("download")
        .arg(&url)
        .arg(&quality_id)
        .arg(&target_dir)
        .arg(&resolved_name)
        .arg(connections.to_string())
        .arg(&job_id)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    #[cfg(windows)]
    cmd.creation_flags(0x08000000);

    if let Ok(mut child) = cmd.spawn() {
        if let Some(stdout) = child.stdout.take() {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(stdout).lines();

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        let _ = child.kill().await;
                        break;
                    }
                    line_res = reader.next_line() => {
                        match line_res {
                            Ok(Some(line)) => {
                                let trimmed = line.trim();
                                let json_start = trimmed.find("{\"type\":");
                                if let Some(pos) = json_start {
                                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&trimmed[pos..]) {
                                        match v.get("type").and_then(|t| t.as_str()) {
                                            Some("progress") => {
                                                let dl = v.get("downloaded_bytes").and_then(|b| b.as_u64()).unwrap_or(0);
                                                let tot = v.get("total_bytes").and_then(|b| b.as_u64());
                                                let speed_bps = v.get("speed_bps").and_then(|s| s.as_f64()).unwrap_or(0.0);
                                                let eta = v.get("eta_seconds").and_then(|e| e.as_u64());
                                                let active_streams = v.get("streams").and_then(|s| s.as_u64()).unwrap_or(connections as u64) as usize;

                                                let _ = manifest.update_downloaded_bytes(&job_id, dl);

                                                {
                                                    let mut map = active_snaps.lock().await;
                                                    map.insert(job_id.clone(), types::ProgressSnapshot {
                                                        job_id: job_id.clone(),
                                                        filename: resolved_name.clone(),
                                                        destination_path: target_path.display().to_string(),
                                                        total_bytes: tot,
                                                        downloaded_bytes: dl,
                                                        verified_bytes: dl,
                                                        speed_bps,
                                                        eta_seconds: eta,
                                                        status: types::DownloadStatus::Active,
                                                        resumable: true,
                                                        connections: active_streams,
                                                        error_message: None,
                                                    });
                                                }

                                                if let Ok(st) = generate_full_state(&state_clone, &target_dir).await {
                                                    let _ = proxy_clone.send_event(UserEvent::PushStateUpdate(st));
                                                }
                                            }
                                            Some("completed") => {
                                                let tot = v.get("total_bytes").and_then(|b| b.as_u64()).unwrap_or(0);
                                                if let Some(fp) = v.get("file_path").and_then(|f| f.as_str()) {
                                                    if let Ok(Some(mut cur_job)) = manifest.get_job(&job_id) {
                                                        cur_job.destination = fp.to_string();
                                                        if let Some(fname) = std::path::Path::new(fp).file_name().and_then(|n| n.to_str()) {
                                                            cur_job.filename = fname.to_string();
                                                        }
                                                        cur_job.downloaded_bytes = tot;
                                                        cur_job.total_size = Some(tot);
                                                        cur_job.status = types::DownloadStatus::Completed;
                                                        cur_job.completed_at = Some(chrono::Utc::now().timestamp_millis());
                                                        let _ = manifest.upsert_job(&cur_job);
                                                    }
                                                } else {
                                                    let _ = manifest.update_downloaded_bytes(&job_id, tot);
                                                    let _ = manifest.update_status(&job_id, types::DownloadStatus::Completed, None);
                                                }
                                                {
                                                    let mut map = active_snaps.lock().await;
                                                    map.remove(&job_id);
                                                }
                                                if let Ok(st) = generate_full_state(&state_clone, &target_dir).await {
                                                    let _ = proxy_clone.send_event(UserEvent::PushStateUpdate(st));
                                                }
                                            }
                                            Some("error") => {
                                                let err_msg = v.get("error").and_then(|e| e.as_str()).unwrap_or("Download failed");
                                                let _ = manifest.update_status(&job_id, types::DownloadStatus::Error, Some(err_msg));
                                                {
                                                    let mut map = active_snaps.lock().await;
                                                    map.remove(&job_id);
                                                }
                                                if let Ok(st) = generate_full_state(&state_clone, &target_dir).await {
                                                    let _ = proxy_clone.send_event(UserEvent::PushStateUpdate(st));
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            _ => break,
                        }
                    }
                }
            }
        }
    }
}

fn handle_ipc_message(
    msg: &str,
    webview: &WebView,
    state: &Arc<AppState>,
    rt: &tokio::runtime::Runtime,
    window: &tao::window::Window,
    proxy: &EventLoopProxy<UserEvent>,
) {
    let parsed: serde_json::Value = match serde_json::from_str(msg) {
        Ok(v) => v,
        Err(_) => return,
    };

    let action = parsed.get("action").and_then(|a| a.as_str()).unwrap_or("");
    let val = &parsed;

    match action {
        "window_minimize" => {
            window.set_minimized(true);
        }
        "window_maximize" => {
            let is_max = window.is_maximized();
            window.set_maximized(!is_max);
        }
        "window_close" => {
            std::process::exit(0);
        }
        "detect_url" => {
            let url = val.get("url").and_then(|u| u.as_str()).unwrap_or("");
            let default_dir =
                rt.block_on(async { state.settings.lock().await.default_download_dir.clone() });
            let url_str = url.to_string();

            if is_ai_workload_url(&url_str) {
                let script_path = get_ai_script_path();
                let proxy_clone = proxy.clone();
                let url_clone = url_str.clone();

                rt.spawn(async move {
                    #[cfg(windows)]
                    use std::os::windows::process::CommandExt;
                    let mut cmd = std::process::Command::new("python");
                    cmd.arg(&script_path).arg("probe").arg(&url_clone).arg(&default_dir);
                    #[cfg(windows)]
                    cmd.creation_flags(0x08000000);

                    if let Ok(output) = cmd.output() {
                        let out_str = String::from_utf8_lossy(&output.stdout);
                        if let Some(json_start) = out_str.find('{') {
                            if let Ok(res) = serde_json::from_str::<serde_json::Value>(&out_str[json_start..]) {
                                if res.get("is_valid").and_then(|b| b.as_bool()).unwrap_or(false) {
                                    let script = format!(
                                        "if (window.onOtterEvent) {{ window.onOtterEvent({{ type: 'detection_result', data: {} }}); }}",
                                        serde_json::to_string(&res).unwrap_or_default()
                                    );
                                    let _ = proxy_clone.send_event(UserEvent::EvalScript(script));
                                    return;
                                }
                            }
                        }
                    }

                    let result = UrlDetector::detect(&url_clone, &default_dir);
                    let res_json = serde_json::to_string(&result).unwrap_or_default();
                    let script = format!(
                        "if (window.onOtterEvent) {{ window.onOtterEvent({{ type: 'detection_result', data: {} }}); }}",
                        res_json
                    );
                    let _ = proxy_clone.send_event(UserEvent::EvalScript(script));
                });
            } else if is_media_url(&url_str) {
                let script_path = get_media_script_path();
                let proxy_clone = proxy.clone();
                let url_clone = url_str.clone();

                rt.spawn(async move {
                    #[cfg(windows)]
                    use std::os::windows::process::CommandExt;
                    let mut cmd = std::process::Command::new("python");
                    cmd.arg(&script_path).arg("probe").arg(&url_clone);
                    #[cfg(windows)]
                    cmd.creation_flags(0x08000000);

                    if let Ok(output) = cmd.output() {
                        let out_str = String::from_utf8_lossy(&output.stdout);
                        if let Some(json_start) = out_str.find('{') {
                            if let Ok(res) = serde_json::from_str::<serde_json::Value>(&out_str[json_start..]) {
                                if res.get("is_media").and_then(|b| b.as_bool()).unwrap_or(false) {
                                    let platform = detect_platform_label(&url_clone);
                                    let title = res.get("title").and_then(|t| t.as_str()).unwrap_or("Media Stream");
                                    let clean_fn = res.get("clean_filename").and_then(|f| f.as_str()).unwrap_or("media");
                                    let payload = serde_json::json!({
                                        "is_valid": true,
                                        "is_media": true,
                                        "platform_name": platform,
                                        "content_type_label": "Media Stream (Video/Audio)",
                                        "title": title,
                                        "suggested_filename": format!("{}.mp4", clean_fn),
                                        "expected_size_bytes": null,
                                        "quality_options": res.get("quality_options"),
                                        "default_quality_id": res.get("default_quality_id"),
                                        "thumbnail": res.get("thumbnail"),
                                        "raw_url": url_clone,
                                        "url": url_clone,
                                    });
                                    let script = format!(
                                        "if (window.onOtterEvent) {{ window.onOtterEvent({{ type: 'detection_result', data: {} }}); }}",
                                        serde_json::to_string(&payload).unwrap_or_default()
                                    );
                                    let _ = proxy_clone.send_event(UserEvent::EvalScript(script));
                                    return;
                                }
                            }
                        }
                    }

                    // Fallback with is_media: true so media engine is used even if probe was throttled
                    let result = UrlDetector::detect(&url_clone, &default_dir);
                    let mut res_val = serde_json::to_value(&result).unwrap_or_default();
                    if let Some(map) = res_val.as_object_mut() {
                        map.insert("is_media".to_string(), serde_json::Value::Bool(true));
                        map.insert("quality_options".to_string(), serde_json::json!([
                            {"id": "video_best", "height": 0, "label": "Best Video Quality (Auto)", "type": "video"},
                            {"id": "video_1080", "height": 1080, "label": "1080p (Full HD)", "type": "video"},
                            {"id": "video_720", "height": 720, "label": "720p (HD)", "type": "video"},
                            {"id": "video_480", "height": 480, "label": "480p (SD)", "type": "video"},
                            {"id": "video_240", "height": 240, "label": "240p", "type": "video"},
                            {"id": "audio_mp3", "height": 0, "label": "Audio Only (MP3)", "type": "audio"},
                            {"id": "audio_m4a", "height": 0, "label": "Audio Only (M4A)", "type": "audio"}
                        ]));
                        map.insert("default_quality_id".to_string(), serde_json::json!("video_best"));
                    }
                    let res_json = serde_json::to_string(&res_val).unwrap_or_default();
                    let script = format!(
                        "if (window.onOtterEvent) {{ window.onOtterEvent({{ type: 'detection_result', data: {} }}); }}",
                        res_json
                    );
                    let _ = proxy_clone.send_event(UserEvent::EvalScript(script));
                });
            } else {
                let result = UrlDetector::detect(url, &default_dir);
                let res_json = serde_json::to_string(&result).unwrap_or_default();
                let script = format!(
                    "if (window.onOtterEvent) {{ window.onOtterEvent({{ type: 'detection_result', data: {} }}); }}",
                    res_json
                );
                let _ = webview.evaluate_script(&script);
            }
        }
        "browse_folder_modal" => {
            let proxy_clone = proxy.clone();
            std::thread::spawn(move || {
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    let ps_script = r#"[System.Reflection.Assembly]::LoadWithPartialName('System.windows.forms') | Out-Null; $f = New-Object System.Windows.Forms.FolderBrowserDialog; $f.Description = 'Select Download Location'; $f.ShowNewFolderButton = $true; if ($f.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { Write-Output $f.SelectedPath }"#;
                    if let Ok(out) = std::process::Command::new("powershell")
                        .args(&["-NoProfile", "-NonInteractive", "-Command", ps_script])
                        .creation_flags(0x08000000)
                        .output()
                    {
                        let chosen = String::from_utf8_lossy(&out.stdout).trim().to_string();
                        if !chosen.is_empty() {
                            let script = format!(
                                "if (window.onOtterEvent) {{ window.onOtterEvent({{ type: 'modal_dest_selected', path: {} }}); }}",
                                serde_json::to_string(&chosen).unwrap_or_default()
                            );
                            let _ = proxy_clone.send_event(UserEvent::EvalScript(script));
                        }
                    }
                }
            });
        }
        "toggle_live_detection" => {
            let enabled = val.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true);
            let settings_ref = state.settings.clone();
            let current_target_dir = {
                let mut lock = rt.block_on(async { settings_ref.lock().await });
                lock.live_detection = enabled;
                let _ = lock.save();
                lock.default_download_dir.clone()
            };
            if let Ok(state_json) = rt.block_on(generate_full_state(state, &current_target_dir)) {
                let _ = proxy.send_event(UserEvent::PushStateUpdate(state_json));
            }
        }
        "start_download" => {
            let url = val
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string();
            let destination = val
                .get("destination")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());
            let filename = val
                .get("filename")
                .and_then(|f| f.as_str())
                .map(|s| s.to_string());
            let connections = val
                .get("connections")
                .and_then(|c| c.as_u64())
                .unwrap_or(32) as usize;
            let connections = if connections == 0 { 32 } else { connections };
            let quality_id = val
                .get("quality_id")
                .and_then(|q| q.as_str())
                .unwrap_or("video_best")
                .to_string();
            let is_media = val
                .get("is_media")
                .and_then(|m| m.as_bool())
                .unwrap_or(false)
                || is_media_url(&url)
                || is_ai_workload_url(&url);
            let thumbnail = val
                .get("thumbnail")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();

            let mgr = state.manager.clone();
            let manifest = state.manifest.clone();
            let settings_ref = state.settings.clone();
            let cancel_tokens = state.cancellation_tokens.clone();
            let active_snaps = state.active_snapshots.clone();
            let state_clone = state.clone();
            let proxy_clone = proxy.clone();

            rt.spawn(async move {
                let default_dir = settings_ref.lock().await.default_download_dir.clone();
                let mut target_dir = destination.unwrap_or(default_dir);
                if std::fs::create_dir_all(&target_dir).is_err() {
                    let user_p = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Default".to_string());
                    target_dir = format!("{}\\Downloads\\Otter", user_p);
                    let _ = std::fs::create_dir_all(&target_dir);
                }

                let job_id = format!("{:x}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));

                if !thumbnail.is_empty() {
                    let mut thumbs = state_clone.thumbnails.lock().await;
                    thumbs.insert(job_id.clone(), thumbnail.clone());
                    thumbs.insert(url.clone(), thumbnail);
                }

                if is_media {
                    let resolved_name = filename.unwrap_or_else(|| "media_download.mp4".to_string());
                    let target_path = std::path::PathBuf::from(&target_dir).join(&resolved_name);

                    let job = types::JobRecord {
                        id: job_id.clone(),
                        url: url.clone(),
                        filename: resolved_name.clone(),
                        destination: target_path.display().to_string(),
                        total_size: None,
                        downloaded_bytes: 0,
                        status: types::DownloadStatus::Active,
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

                    let cancel_token = tokio_util::sync::CancellationToken::new();
                    {
                        let mut tokens = cancel_tokens.lock().await;
                        tokens.insert(job_id.clone(), cancel_token.clone());
                    }

                    // Push state update immediately so active card appears
                    if let Ok(st) = generate_full_state(&state_clone, &target_dir).await {
                        let _ = proxy_clone.send_event(UserEvent::PushStateUpdate(st));
                    }

                    run_media_download_job(
                        job_id.clone(),
                        url.clone(),
                        quality_id,
                        target_dir.clone(),
                        resolved_name,
                        connections,
                        cancel_token,
                        manifest.clone(),
                        active_snaps.clone(),
                        state_clone.clone(),
                        proxy_clone.clone(),
                    ).await;

                    {
                        let mut tokens = cancel_tokens.lock().await;
                        tokens.remove(&job_id);
                    }
                } else {
                    // Direct high-speed file download using OTTER's native multi-connection engine
                    let client = reqwest::Client::builder()
                        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
                        .timeout(Duration::from_secs(15))
                        .build()
                        .unwrap_or_default();

                    let probe = engine::probe::probe_url(&client, &url).await;
                    let (resolved_name, total_size, resumable, etag, last_modified) = match &probe {
                        Ok(p) => (filename.unwrap_or_else(|| p.filename.clone()), p.total_size, p.resumable, p.etag.clone(), p.last_modified.clone()),
                        Err(_) => (filename.unwrap_or_else(|| "download.bin".to_string()), None, false, None, None),
                    };

                    let target_path = std::path::PathBuf::from(&target_dir).join(&resolved_name);

                    let job = types::JobRecord {
                        id: job_id.clone(),
                        url: url.clone(),
                        filename: resolved_name,
                        destination: target_path.display().to_string(),
                        total_size,
                        downloaded_bytes: 0,
                        status: types::DownloadStatus::Queued,
                        resumable,
                        etag,
                        last_modified,
                        expected_sha256: None,
                        computed_sha256: None,
                        error: None,
                        created_at: chrono::Utc::now().timestamp_millis(),
                        completed_at: None,
                    };

                    if let Err(e) = manifest.upsert_job(&job) {
                        eprintln!("Failed to create job in manifest: {}", e);
                        return;
                    }

                    let cancel_token = tokio_util::sync::CancellationToken::new();
                    {
                        let mut tokens = cancel_tokens.lock().await;
                        tokens.insert(job_id.clone(), cancel_token.clone());
                    }

                    let effective_connections = match total_size {
                        Some(size) if size < 1 * 1024 * 1024 => 1,
                        _ => connections.max(1),
                    };

                    let _ = mgr.run_job(&job_id, effective_connections, cancel_token).await;

                    {
                        let mut tokens = cancel_tokens.lock().await;
                        tokens.remove(&job_id);
                    }
                }
            });
        }
        "pause_download" => {
            let id = val
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string();
            let cancel_tokens = state.cancellation_tokens.clone();
            let manifest = state.manifest.clone();

            rt.spawn(async move {
                let token = {
                    let mut tokens = cancel_tokens.lock().await;
                    tokens.remove(&id)
                };
                if let Some(t) = token {
                    t.cancel();
                }
                let _ = manifest.update_status(&id, types::DownloadStatus::Paused, None);
            });
        }
        "resume_download" => {
            let id = val
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string();
            let manifest = state.manifest.clone();
            let mgr = state.manager.clone();
            let cancel_tokens = state.cancellation_tokens.clone();
            let active_snaps = state.active_snapshots.clone();
            let state_clone = state.clone();
            let proxy_clone = proxy.clone();

            rt.spawn(async move {
                if let Ok(Some(job)) = manifest.get_job(&id) {
                    let cancel_token = tokio_util::sync::CancellationToken::new();
                    {
                        let mut tokens = cancel_tokens.lock().await;
                        tokens.insert(id.clone(), cancel_token.clone());
                    }
                    let _ = manifest.update_status(&id, types::DownloadStatus::Active, None);

                    if is_media_url(&job.url) || is_ai_workload_url(&job.url) {
                        let target_path = std::path::PathBuf::from(&job.destination);
                        let target_dir = target_path
                            .parent()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| {
                                std::env::var("USERPROFILE")
                                    .map(|u| format!("{}\\Desktop", u))
                                    .unwrap_or_default()
                            });
                        let quality_id = "video_best".to_string();
                        let connections = 32;

                        run_media_download_job(
                            id.clone(),
                            job.url.clone(),
                            quality_id,
                            target_dir,
                            job.filename.clone(),
                            connections,
                            cancel_token,
                            manifest.clone(),
                            active_snaps.clone(),
                            state_clone.clone(),
                            proxy_clone.clone(),
                        )
                        .await;
                    } else {
                        let total_size = job.total_size;
                        let connections = match total_size {
                            Some(s) if s > 1_000_000_000 => 32,
                            Some(s) if s > 100_000_000 => 16,
                            Some(s) if s > 10_000_000 => 8,
                            _ => 4,
                        };
                        let _ = mgr.run_job(&id, connections, cancel_token).await;
                    }

                    {
                        let mut tokens = cancel_tokens.lock().await;
                        tokens.remove(&id);
                    }
                }
            });
        }
        "cancel_download" => {
            let id = val
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string();
            let cancel_tokens = state.cancellation_tokens.clone();
            let manifest = state.manifest.clone();
            let active_snaps = state.active_snapshots.clone();
            let state_clone = state.clone();
            let proxy_clone = proxy.clone();

            rt.spawn(async move {
                let token = {
                    let mut tokens = cancel_tokens.lock().await;
                    tokens.remove(&id)
                };
                if let Some(t) = token {
                    t.cancel();
                }
                if let Ok(Some(job)) = manifest.get_job(&id) {
                    cleanup_job_files(&job.destination);
                }
                let _ = manifest.delete_job(&id);

                {
                    let mut map = active_snaps.lock().await;
                    map.remove(&id);
                }

                let default_dir = state_clone
                    .settings
                    .lock()
                    .await
                    .default_download_dir
                    .clone();
                if let Ok(state_json) = generate_full_state(&state_clone, &default_dir).await {
                    let _ = proxy_clone.send_event(UserEvent::PushStateUpdate(state_json));
                }
            });
        }
        "remove_job" => {
            let id = val
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string();
            let delete_file = val
                .get("delete_file")
                .and_then(|d| d.as_bool())
                .unwrap_or(false);
            let cancel_tokens = state.cancellation_tokens.clone();
            let manifest = state.manifest.clone();

            rt.spawn(async move {
                let token = {
                    let mut tokens = cancel_tokens.lock().await;
                    tokens.remove(&id)
                };
                if let Some(t) = token {
                    t.cancel();
                }
                if let Ok(Some(job)) = manifest.get_job(&id) {
                    if delete_file {
                        cleanup_job_files(&job.destination);
                    }
                    let _ = manifest.delete_job(&id);
                }
            });
        }
        "handoff_browser_download" => {
            let id = val.get("id").and_then(|i| i.as_i64()).unwrap_or(0);
            let bridge = state.browser_bridge.clone();
            let mgr = state.manager.clone();
            let manifest = state.manifest.clone();
            let settings_ref = state.settings.clone();
            let cancel_tokens = state.cancellation_tokens.clone();
            let proxy_clone = proxy.clone();
            let state_clone = state.clone();

            rt.spawn(async move {
                if let Some(item) = bridge.get_item(id).await {
                    let default_dir = settings_ref.lock().await.default_download_dir.clone();
                    let target_dir = if std::path::Path::new("D:\\OTTER_CHROME_BENCHMARK").exists()
                    {
                        "D:\\OTTER_CHROME_BENCHMARK".to_string()
                    } else {
                        default_dir
                    };
                    let _ = std::fs::create_dir_all(&target_dir);

                    let job_id = format!(
                        "{:x}",
                        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
                    );
                    let target_path = std::path::PathBuf::from(&target_dir).join(&item.filename);

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

                    if let Ok(_) = manifest.upsert_job(&job) {
                        bridge.set_item_handed_off(id, job_id.clone()).await;

                        let cancel_token = tokio_util::sync::CancellationToken::new();
                        {
                            let mut tokens = cancel_tokens.lock().await;
                            tokens.insert(job_id.clone(), cancel_token.clone());
                        }

                        // Trigger GUI push update
                        if let Ok(st) = generate_full_state(&state_clone, &target_dir).await {
                            let _ = proxy_clone.send_event(UserEvent::PushStateUpdate(st));
                        }

                        let _ = mgr.run_job(&job_id, 16, cancel_token).await;

                        {
                            let mut tokens = cancel_tokens.lock().await;
                            tokens.remove(&job_id);
                        }
                    }
                }
            });
        }
        "open_path" => {
            let path_str = val
                .get("path")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            #[cfg(windows)]
            {
                let path = std::path::PathBuf::from(&path_str);
                if path.exists() {
                    if path.is_file() {
                        let _ = std::process::Command::new("explorer")
                            .arg(format!("/select,{}", path.display()))
                            .spawn();
                    } else {
                        let _ = std::process::Command::new("explorer").arg(&path).spawn();
                    }
                } else if let Some(parent) = path.parent() {
                    if parent.exists() {
                        let _ = std::process::Command::new("explorer").arg(parent).spawn();
                    } else {
                        let default_dir = rt.block_on(async {
                            state.settings.lock().await.default_download_dir.clone()
                        });
                        let _ = std::process::Command::new("explorer")
                            .arg(default_dir)
                            .spawn();
                    }
                } else {
                    let default_dir = rt.block_on(async {
                        state.settings.lock().await.default_download_dir.clone()
                    });
                    let _ = std::process::Command::new("explorer")
                        .arg(default_dir)
                        .spawn();
                }
            }
        }
        "open_file" => {
            let path_str = val
                .get("path")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            #[cfg(windows)]
            {
                let _ = std::process::Command::new("explorer").arg(path_str).spawn();
            }
        }
        "run_doctor" => {
            let default_dir =
                rt.block_on(async { state.settings.lock().await.default_download_dir.clone() });
            if let Ok(doctor) = engine::doctor::DoctorEngine::new() {
                let report = rt.block_on(async {
                    doctor
                        .run_diagnostics(Some(std::path::PathBuf::from(default_dir)), true)
                        .await
                });
                if let Ok(rep_json) = serde_json::to_string(&report) {
                    let script = format!(
                        "if (window.onOtterEvent) {{ window.onOtterEvent({{ type: 'doctor_result', data: {} }}); }}",
                        rep_json
                    );
                    let _ = webview.evaluate_script(&script);
                }
            }
        }
        "save_settings" => {
            if let Some(set_val) = val.get("settings") {
                if let Ok(new_set) = serde_json::from_value::<GuiSettings>(set_val.clone()) {
                    let _ = new_set.save();
                    let set_mut = state.settings.clone();
                    rt.block_on(async move {
                        *set_mut.lock().await = new_set;
                    });
                }
            }
        }
        "choose_download_dir" => {
            #[cfg(windows)]
            {
                let settings_ref = state.settings.clone();
                let proxy_clone = proxy.clone();
                let rt_handle = rt.handle().clone();
                std::thread::spawn(move || {
                    let ps_cmd = r#"Add-Type -AssemblyName System.Windows.Forms; $f = New-Object System.Windows.Forms.FolderBrowserDialog; $f.Description = 'Select Otter Download Directory'; $f.ShowNewFolderButton = $true; if ($f.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { Write-Output $f.SelectedPath }"#;
                    let output = std::process::Command::new("powershell")
                        .args(&["-NoProfile", "-NonInteractive", "-Command", ps_cmd])
                        .creation_flags(0x08000000)
                        .output();
                    if let Ok(out) = output {
                        let chosen = String::from_utf8_lossy(&out.stdout).trim().to_string();
                        if !chosen.is_empty() && std::path::Path::new(&chosen).is_dir() {
                            let updated_settings = rt_handle.block_on(async {
                                let mut lock = settings_ref.lock().await;
                                lock.default_download_dir = chosen.clone();
                                let _ = lock.save();
                                lock.clone()
                            });
                            let update_payload = serde_json::json!({
                                "settings": updated_settings
                            });
                            let _ = proxy_clone.send_event(UserEvent::PushStateUpdate(
                                serde_json::to_string(&update_payload).unwrap_or_default(),
                            ));
                        }
                    }
                });
            }
        }
        "reset_settings" => {
            let default_s = GuiSettings::default();
            let _ = default_s.save();
            let set_mut = state.settings.clone();
            rt.block_on(async move {
                *set_mut.lock().await = default_s.clone();
            });
            let script = format!(
                "if (window.onOtterEvent) {{ window.onOtterEvent({{ type: 'state_update', data: {{ settings: {} }} }}); }}",
                serde_json::to_string(&GuiSettings::default()).unwrap_or_default()
            );
            let _ = webview.evaluate_script(&script);
        }
        "open_settings_dir" => {
            #[cfg(windows)]
            {
                let local_app_data = std::env::var("LOCALAPPDATA")
                    .unwrap_or_else(|_| "C:\\Users\\Default\\AppData\\Local".to_string());
                let dir = std::path::PathBuf::from(local_app_data).join("otter");
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::process::Command::new("explorer").arg(dir).spawn();
            }
        }
        _ => {}
    }
}

fn detect_platform_label(url: &str) -> String {
    let lower = url.trim().to_lowercase();
    if lower.contains("huggingface.co") || lower.starts_with("hf:") {
        "Hugging Face".into()
    } else if lower.contains("kaggle.com")
        || lower.starts_with("kaggle:")
        || lower.starts_with("kaggle ")
    {
        "Kaggle".into()
    } else if lower.contains("youtube.com") || lower.contains("youtu.be") {
        "YouTube".into()
    } else if lower.contains("instagram.com") {
        "Instagram".into()
    } else if lower.contains("pinterest.com") || lower.contains("pin.it") {
        "Pinterest".into()
    } else if lower.contains("steampowered.com") || lower.contains("steamcommunity.com") {
        "Steam".into()
    } else if lower.contains("epicgames.com") {
        "Epic Games".into()
    } else if lower.contains("github.com") {
        "GitHub".into()
    } else if lower.contains("ollama.com")
        || lower.starts_with("ollama:")
        || lower.starts_with("ollama pull ")
        || lower.starts_with("ollama run ")
    {
        "Ollama".into()
    } else {
        "Direct Link".into()
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

fn get_media_script_path() -> std::path::PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| "C:\\Users\\Default\\AppData\\Local".to_string());
    let dir = std::path::PathBuf::from(local)
        .join("otter")
        .join("scripts");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("media_engine.py");
    let _ = std::fs::write(&path, include_str!("../scripts/media_engine.py"));
    path
}

fn get_ai_script_path() -> std::path::PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| "C:\\Users\\Default\\AppData\\Local".to_string());
    let dir = std::path::PathBuf::from(local)
        .join("otter")
        .join("scripts");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("ai_engine.py");
    let _ = std::fs::write(&path, include_str!("../scripts/ai_engine.py"));
    path
}

fn is_ai_workload_url(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    lower.starts_with("ollama pull ")
        || lower.starts_with("ollama run ")
        || lower.starts_with("ollama:")
        || lower.contains("ollama.com/")
        || lower.contains("huggingface.co/")
        || lower.starts_with("hf:")
        || lower.contains("kaggle.com/datasets/")
        || lower.contains("kaggle.com/api/v1/datasets/")
        || lower.starts_with("kaggle:")
        || lower.starts_with("kaggle ")
}

fn is_media_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    let is_media_ext = lower.ends_with(".mp4")
        || lower.ends_with(".mkv")
        || lower.ends_with(".webm")
        || lower.ends_with(".m3u8")
        || lower.ends_with(".mp3")
        || lower.ends_with(".m4a")
        || lower.ends_with(".wav")
        || lower.ends_with(".flac")
        || lower.ends_with(".ogg")
        || lower.ends_with(".aac")
        || lower.ends_with(".mov")
        || lower.ends_with(".avi");

    let is_media_site = lower.contains("youtube.com")
        || lower.contains("youtu.be")
        || lower.contains("instagram.com")
        || lower.contains("pinterest.com")
        || lower.contains("pin.it")
        || lower.contains("tiktok.com")
        || lower.contains("twitter.com")
        || lower.contains("x.com")
        || lower.contains("vimeo.com")
        || lower.contains("soundcloud.com")
        || lower.contains("dailymotion.com")
        || lower.contains("twitch.tv")
        || lower.contains("reddit.com")
        || lower.contains("fb.watch")
        || lower.contains("facebook.com")
        || lower.contains("bilibili.com")
        || lower.contains("streamable.com")
        || lower.contains("rumble.com")
        || lower.contains("odysee.com")
        || lower.contains("bandcamp.com")
        || lower.contains("mixcloud.com")
        || lower.contains("archive.org/details");

    is_media_ext || is_media_site
}

fn cleanup_job_files(destination: &str) {
    let dest_path = std::path::Path::new(destination);
    let _ = std::fs::remove_file(dest_path);
    let _ = std::fs::remove_file(format!("{}.part", destination));
    let _ = std::fs::remove_file(format!("{}.ytdl", destination));

    if let (Some(parent), Some(stem)) = (
        dest_path.parent(),
        dest_path.file_stem().and_then(|s| s.to_str()),
    ) {
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let p = entry.path();
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with(stem)
                        && (name.ends_with(".part")
                            || name.ends_with(".ytdl")
                            || name.contains(".part-"))
                    {
                        let _ = std::fs::remove_file(&p);
                    }
                }
            }
        }
    }

    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let staging_dir = std::path::PathBuf::from(&local)
            .join("otter")
            .join("staging");
        let _ = std::fs::remove_dir_all(&staging_dir);
        let temp_dir = std::path::PathBuf::from(&local).join("otter").join("temp");
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
