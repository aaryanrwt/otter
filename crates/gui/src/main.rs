#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod detector;
mod state;

use anyhow::Result;
use detector::UrlDetector;
use state::{AppState, GuiSettings};
use std::sync::Arc;
use std::time::Duration;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::http::{Response, header::CONTENT_TYPE};
use wry::{WebView, WebViewBuilder};

const INDEX_HTML: &str = include_str!("../ui/index.html");
const STYLE_CSS: &str = include_str!("../ui/style.css");
const APP_JS: &str = include_str!("../ui/app.js");

const MASCOT_FLYING: &[u8] = include_bytes!("../../../images/Flying Otter Mascot with Flowing Scarf.png");
const MASCOT_FOCUSED: &[u8] = include_bytes!("../../../images/Focused Otter with Laptop and Coffee.png");
const MASCOT_CHEERFUL: &[u8] = include_bytes!("../../../images/Cheerful Otter Popping Out of Pink Box.png");
const MASCOT_SLEEPING: &[u8] = include_bytes!("../../../images/Sleeping Otter with Pink Paw Pillow.png");

enum UserEvent {
    IpcMessage(String),
    PushStateUpdate(String),
}

fn main() -> Result<()> {
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

    // Custom protocol handler for otter:// schema
    let handler_proxy = proxy.clone();
    let webview = WebViewBuilder::new()
        .with_custom_protocol("otter".into(), move |_webview, request| {
            let uri = request.uri().to_string().to_lowercase();
            if uri.contains("flying") {
                Response::builder()
                    .header(CONTENT_TYPE, "image/png")
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Cache-Control", "public, max-age=86400")
                    .body(MASCOT_FLYING.into())
                    .unwrap()
            } else if uri.contains("focused") {
                Response::builder()
                    .header(CONTENT_TYPE, "image/png")
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Cache-Control", "public, max-age=86400")
                    .body(MASCOT_FOCUSED.into())
                    .unwrap()
            } else if uri.contains("cheerful") {
                Response::builder()
                    .header(CONTENT_TYPE, "image/png")
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Cache-Control", "public, max-age=86400")
                    .body(MASCOT_CHEERFUL.into())
                    .unwrap()
            } else if uri.contains("sleeping") {
                Response::builder()
                    .header(CONTENT_TYPE, "image/png")
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Cache-Control", "public, max-age=86400")
                    .body(MASCOT_SLEEPING.into())
                    .unwrap()
            } else if uri.contains("style.css") {
                Response::builder()
                    .header(CONTENT_TYPE, "text/css")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(STYLE_CSS.as_bytes().into())
                    .unwrap()
            } else if uri.contains("app.js") {
                Response::builder()
                    .header(CONTENT_TYPE, "application/javascript")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(APP_JS.as_bytes().into())
                    .unwrap()
            } else {
                Response::builder()
                    .header(CONTENT_TYPE, "text/html")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(INDEX_HTML.as_bytes().into())
                    .unwrap()
            }
        })
        .with_ipc_handler(move |msg: wry::http::Request<String>| {
            let _ = handler_proxy.send_event(UserEvent::IpcMessage(msg.body().clone()));
        })
        .with_url("otter://app/index.html")
        .build(&window)?;

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
                    handle_ipc_message(&msg, &webview, &app_state, &tokio_rt, &window);
                }
                UserEvent::PushStateUpdate(json_str) => {
                    let script = format!(
                        "if (window.onOtterEvent) {{ window.onOtterEvent({{ type: 'state_update', data: {} }}); }}",
                        json_str
                    );
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

fn app_state_init(_rt: &tokio::runtime::Runtime) -> Result<AppState> {
    AppState::new()
}

async fn generate_full_state(
    app_state: &AppState,
    current_target_dir: &str,
) -> Result<String> {
    let jobs = app_state.manifest.list_jobs()?;
    let snap_map = app_state.active_snapshots.lock().await.clone();

    let mut active_jobs = Vec::new();
    let mut total_speed_bps = 0.0;
    let mut total_downloaded_bytes: u64 = 0;

    for j in &jobs {
        let mut speed_str = "0 MB/s".to_string();
        let mut progress_pct = 0.0;
        let mut status_str = j.status.to_string();

        if let Some(snap) = snap_map.get(&j.id) {
            speed_str = format!("{:.1} MB/s", snap.speed_bps / (1024.0 * 1024.0));
            progress_pct = snap.percentage();
            status_str = snap.status.to_string();
            total_speed_bps += snap.speed_bps;
        } else if let Some(total) = j.total_size {
            if total > 0 {
                progress_pct = ((j.downloaded_bytes as f64 / total as f64) * 100.0).clamp(0.0, 100.0);
            }
        }

        total_downloaded_bytes += j.downloaded_bytes;

        let total_size_str = j
            .total_size
            .map(format_bytes)
            .unwrap_or_else(|| "Unknown".to_string());

        let job_obj = serde_json::json!({
            "id": j.id,
            "filename": j.filename,
            "source_url": j.url,
            "platform": detect_platform_label(&j.url),
            "status": status_str,
            "total_bytes": j.total_size,
            "total_size_str": total_size_str,
            "downloaded_bytes": j.downloaded_bytes,
            "progress_pct": progress_pct,
            "speed_str": speed_str,
            "destination_path": j.destination,
        });

        if j.status == types::DownloadStatus::Active || snap_map.contains_key(&j.id) {
            active_jobs.push(job_obj.clone());
        }
    }

    let active_count = active_jobs.len();
    let completed_count = jobs.iter().filter(|j| j.status == types::DownloadStatus::Completed).count();
    let total_speed_mb = total_speed_bps / (1024.0 * 1024.0);
    let total_dl_gb = (total_downloaded_bytes as f64) / (1024.0 * 1024.0 * 1024.0);

    let system_specs = app_state.get_system_specs(current_target_dir);

    let state_payload = serde_json::json!({
        "jobs": jobs.iter().map(|j| {
            serde_json::json!({
                "id": j.id,
                "filename": j.filename,
                "source_url": j.url,
                "status": j.status.to_string(),
                "total_bytes": j.total_size,
                "total_size_str": j.total_size.map(format_bytes).unwrap_or_else(|| "Unknown".into()),
                "downloaded_bytes": j.downloaded_bytes,
                "progress_pct": if let Some(tot) = j.total_size {
                    if tot > 0 { ((j.downloaded_bytes as f64 / tot as f64) * 100.0).clamp(0.0, 100.0) } else { 0.0 }
                } else { 0.0 },
                "speed_str": "0 MB/s",
                "destination_path": j.destination,
            })
        }).collect::<Vec<_>>(),
        "active_jobs": active_jobs,
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

fn handle_ipc_message(
    msg: &str,
    webview: &WebView,
    state: &Arc<AppState>,
    rt: &tokio::runtime::Runtime,
    window: &tao::window::Window,
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
            let default_dir = rt.block_on(async { state.settings.lock().await.default_download_dir.clone() });
            let result = UrlDetector::detect(url, &default_dir);
            let res_json = serde_json::to_string(&result).unwrap_or_default();

            let script = format!("if (window.onOtterEvent) {{ window.onOtterEvent({{ type: 'detection_result', data: {} }}); }}", res_json);
            let _ = webview.evaluate_script(&script);
        }
        "start_download" => {
            let url = val.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string();
            let destination = val.get("destination").and_then(|d| d.as_str()).map(|s| s.to_string());
            let filename = val.get("filename").and_then(|f| f.as_str()).map(|s| s.to_string());
            let connections = val.get("connections").and_then(|c| c.as_u64()).unwrap_or(8) as usize;

            let mgr = state.manager.clone();
            let manifest = state.manifest.clone();
            let settings_ref = state.settings.clone();
            let cancel_tokens = state.cancellation_tokens.clone();

            rt.spawn(async move {
                let default_dir = settings_ref.lock().await.default_download_dir.clone();
                let target_dir = destination.unwrap_or(default_dir);
                let _ = std::fs::create_dir_all(&target_dir);

                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(15))
                    .build()
                    .unwrap_or_default();

                let probe = engine::probe::probe_url(&client, &url).await;
                let (resolved_name, total_size, resumable, etag, last_modified) = match &probe {
                    Ok(p) => (filename.unwrap_or_else(|| p.filename.clone()), p.total_size, p.resumable, p.etag.clone(), p.last_modified.clone()),
                    Err(_) => (filename.unwrap_or_else(|| "download.bin".to_string()), None, false, None, None),
                };

                let target_path = std::path::PathBuf::from(&target_dir).join(&resolved_name);
                let job_id = format!("{:x}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));

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
                    Some(size) if size < 5 * 1024 * 1024 => 1,
                    Some(size) if size < 30 * 1024 * 1024 => connections.min(4),
                    _ => connections.max(1),
                };

                let _ = mgr.run_job(&job_id, effective_connections, cancel_token).await;

                {
                    let mut tokens = cancel_tokens.lock().await;
                    tokens.remove(&job_id);
                }
            });
        }
        "pause_download" => {
            let id = val.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
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
            let id = val.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
            let manifest = state.manifest.clone();
            let mgr = state.manager.clone();
            let cancel_tokens = state.cancellation_tokens.clone();

            rt.spawn(async move {
                if let Ok(Some(_)) = manifest.get_job(&id) {
                    let cancel_token = tokio_util::sync::CancellationToken::new();
                    {
                        let mut tokens = cancel_tokens.lock().await;
                        tokens.insert(id.clone(), cancel_token.clone());
                    }
                    let _ = mgr.run_job(&id, 8, cancel_token).await;
                    {
                        let mut tokens = cancel_tokens.lock().await;
                        tokens.remove(&id);
                    }
                }
            });
        }
        "cancel_download" => {
            let id = val.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
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
                    let part_path = format!("{}.part", job.destination);
                    let _ = std::fs::remove_file(part_path);
                    let _ = manifest.update_status(&id, types::DownloadStatus::Error, Some("Cancelled by user"));
                }
            });
        }
        "remove_job" => {
            let id = val.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
            let delete_file = val.get("delete_file").and_then(|d| d.as_bool()).unwrap_or(false);
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
                        let _ = std::fs::remove_file(&job.destination);
                        let _ = std::fs::remove_file(format!("{}.part", job.destination));
                    }
                    let _ = manifest.delete_job(&id);
                }
            });
        }
        "open_path" => {
            let path_str = val.get("path").and_then(|p| p.as_str()).unwrap_or("").to_string();
            #[cfg(windows)]
            {
                let path = std::path::PathBuf::from(path_str);
                let folder = if path.is_file() {
                    path.parent().map(|p| p.to_path_buf()).unwrap_or(path)
                } else {
                    path
                };
                let _ = std::process::Command::new("explorer")
                    .arg(folder)
                    .spawn();
            }
        }
        "open_file" => {
            let path_str = val.get("path").and_then(|p| p.as_str()).unwrap_or("").to_string();
            #[cfg(windows)]
            {
                let _ = std::process::Command::new("explorer")
                    .arg(path_str)
                    .spawn();
            }
        }
        "run_doctor" => {
            let default_dir = rt.block_on(async { state.settings.lock().await.default_download_dir.clone() });
            if let Ok(doctor) = engine::doctor::DoctorEngine::new() {
                let report = rt.block_on(async {
                    doctor.run_diagnostics(Some(std::path::PathBuf::from(default_dir)), true).await
                });
                if let Ok(rep_json) = serde_json::to_string(&report) {
                    let script = format!("if (window.onOtterEvent) {{ window.onOtterEvent({{ type: 'doctor_result', data: {} }}); }}", rep_json);
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
        _ => {}
    }
}

fn detect_platform_label(url: &str) -> String {
    let lower = url.to_lowercase();
    if lower.contains("huggingface.co") || lower.starts_with("hf:") {
        "Hugging Face".into()
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
    } else if lower.contains("ollama.com") || lower.starts_with("ollama:") {
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
