# Otter GUI feature matrix

Document version: 1.0.0
Engine version: 0.1.0
Author: aaryanrwt

## 1. Core feature matrix

| Feature | Desktop GUI | CLI | Underlying engine component | Status |
| :--- | :--- | :--- | :--- | :--- |
| Universal URL detection | Visual modal with live parsing | Argument parsing (`otter get`) | `crates/gui/src/detector.rs` | Complete |
| SSRF security filter | Active on URL input | Active on CLI inputs | `UrlDetector::validate_url_security` | Complete |
| Multi-stream download | Visual progress bar with speed | Dynamic CLI terminal bar | `crates/engine/src/orchestrator.rs` | Complete |
| AIMD adaptive concurrency | Telemetry card in Engine tab | Console logs / JSON snapshot | `crates/engine/src/adaptive.rs` | Complete |
| Resume verification | Never-Splice validator check | Checkpoint integrity check | `ManifestStore` + ETag validation | Complete |
| Task control (Pause / Resume) | Interactive GUI buttons | Interactive TUI / PID control | `CancellationToken` in `AppState` | Complete |
| Task removal & file wipe | Delete dialog with wipe toggle | `otter clean` / `otter rm` | `ManifestStore::delete_job` | Complete |
| System doctor & diagnostics | One-click button in Engine tab | `otter doctor` command | `engine::doctor::DoctorEngine` | Complete |
| Library classification | File type tabs (Models, Datasets, etc.) | `otter list --filter` | Manifest query + extension matcher | Complete |
| Explorer integration | Direct "Open Folder" / "Open File" | Path display | Win32 Shell API (`explorer.exe`) | Complete |
| Host hardware introspection | Live CPU, RAM, Disk display | System info panel in TUI | Win32 `GlobalMemoryStatusEx` | Complete |
| Persistent settings | Interactive GUI form | Environment variables & config | `%LOCALAPPDATA%\otter\gui_settings.json` | Complete |

---

## 2. Platform support and protocol compatibility

| Target platform / Service | Detection support | Ingestion mechanism | Engine protocol |
| :--- | :--- | :--- | :--- |
| Direct HTTP / HTTPS | Complete | Standard file stream | HTTP/1.1 Range (Multi-socket) |
| Hugging Face Models | Complete | Model card + safetensors / GGUF | HTTP/1.1 Range (Cloudflare CDN) |
| Hugging Face Datasets | Complete | Multi-shard parquet streams | Parallel segment streams |
| Ollama Registry | Complete | Manifest extraction + layer blobs | OCI Registry Layer Streams |
| YouTube / Video streams | Complete | Stream endpoint parser | Segmented media stream |
| Instagram Reels / Media | Complete | Direct media CDN extractor | Direct range stream |
| Pinterest Media | Complete | Static image & video resolver | Direct range stream |
| Steam Store / Community | Complete | App/Depot resolver | Direct package download |
| Epic Games Launcher links | Complete | Manifest parser | Direct package download |
| GitHub Releases & Raw | Complete | Release asset resolver | HTTP/1.1 Range stream |
