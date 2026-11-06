# Otter GUI: architecture audit

Date: 2026-11-06  
Target: Otter v0.1.0 Desktop GUI  
Platform: Windows 10 / Windows 11 (x86_64)  

---

## 1. Existing architecture

The existing Otter repository contains a modular Rust workspace divided into four crates:

- `crates/types`: Domain models for `ByteRange`, `DownloadStatus`, `JobRecord`, `ProgressSnapshot`, and `RemoteFileInfo`.
- `crates/engine`: The core download engine containing:
  - `probe.rs`: HTTP HEAD and Range inspection, detecting byte-range support, content length, and ETag.
  - `orchestrator.rs`: `DownloadManager`, handling async task scheduling, work-stealing, progress broadcasting, and thread execution locking.
  - `adaptive.rs`: AIMD rate controller adapting connection concurrency dynamically.
  - `disk.rs`: Direct-offset positioned writes (`seek_write`), NTFS sparse file preallocation via Win32 `SetFileInformationByHandle`, and Zone.Identifier Mark-of-the-Web streams.
  - `manifest.rs`: SQLite persistence in WAL mode tracking jobs, completed byte ranges, and metadata.
  - `retry.rs`: Error classification and exponential backoff retry policies.
  - `verify.rs`: Streaming incremental SHA-256 calculation across 64 KB buffers.
  - `doctor.rs`: System diagnostics, storage write throughput, and network latency probes.
  - `hf.rs`: Hugging Face repository and dataset inspection, resolving shard download URLs.
  - `ollama.rs`: Ollama model registry client resolving layer digests and physical GGUF blobs.
  - `artifact.rs`: Pure-Rust binary inspection for GGUF, Safetensors, ONNX, and Parquet.
- `crates/cli`: Clap-based command line interface and Crossterm interactive terminal dashboard.
- `crates/harness`: Nine-profile loopback test server simulating various network edge cases.

---

## 2. Existing functionality reusable by the GUI

The GUI does not need to reimplement any download mechanics. The following components are reusable directly:

- Core download execution: `DownloadManager::run_job` handles HTTP Range slicing, concurrent socket streaming, and disk writing.
- Progress reporting: `DownloadManager::subscribe_progress()` broadcasts real-time `ProgressSnapshot` structs over a Tokio channel.
- Persistence and state: `ManifestStore` provides SQLite WAL methods to list, get, pause, resume, cancel, and remove jobs.
- Artifact inspection: `inspect_artifact` provides format inspection for GGUF, Safetensors, ONNX, and Parquet files without loading full weights into RAM.
- Integrity verification: `compute_file_sha256` and `verify_file_integrity` provide cryptographic validation.
- Diagnostics: `DoctorEngine` runs disk write benchmarks, directory access checks, and remote connection tests.
- AI models and datasets: `HfClient` and `OllamaRegistryClient` resolve metadata and layer blobs.

---

## 3. Missing functionality required for the universal GUI

To fulfill the universal download manager vision, the following additions are required:

1. Universal URL detector:
   - A dedicated source adapter subsystem (`crates/engine/src/detector.rs` or `adapter.rs`) that accepts any URL, identifies the platform (YouTube, Instagram, Pinterest, TikTok, Hugging Face, Ollama, Steam, Epic Games, direct archives, media files), and extracts available metadata.
2. URL security and SSRF safeguards:
   - Validation rejecting loopback addresses (`127.0.0.1`, `localhost`), link-local IPs (`169.254.0.0/16`), and private subnets (`10.0.0.0/8`, `192.168.0.0/16`) to protect against server-side request forgery.
   - Path normalization preventing path traversal (`../`) when resolving destination filenames.
3. Media extraction adapters:
   - Metadata extractors for media platforms (YouTube, Instagram, Pinterest) that query title, thumbnail, duration, and stream formats without executing untrusted external scripts.
4. Game store metadata resolvers:
   - Domain resolvers for Steam (`store.steampowered.com/app/...`) and Epic Games (`store.epicgames.com/...`) identifying product names, app IDs, and official client protocol handlers (`steam://install/...`).
5. Desktop application bridge:
   - A native Windows window host using Tauri v2 that bridges the frontend interface to the Rust `DownloadManager` and `ManifestStore`.

---

## 4. Frontend and backend integration

The communication bridge follows a clean asynchronous pattern:

```
+-------------------------------------------------------------+
|                     Desktop Frontend                        |
|        Vanilla HTML5 / Modern CSS / Reactive JavaScript     |
|   Ivory Palette (#FAF7F5), Rounded Cards, Mascot Assets     |
+------------------------------+------------------------------+
                               |
                   Tauri IPC Commands & Events
                               |
+------------------------------v------------------------------+
|                  GUI Application Bridge                      |
|         Tauri v2 Commands (otter_gui / main.rs)             |
|   URL Detector | State Bridge | Event Dispatcher            |
+------------------------------+------------------------------+
                               |
                        In-Process API
                               |
+------------------------------v------------------------------+
|                   Otter Rust Engine Core                    |
|        DownloadManager | ManifestStore | DoctorEngine       |
+-------------------------------------------------------------+
```

### Tauri IPC commands:
- `detect_url(url: String)`: Validates URL, detects source, extracts metadata, and returns available download variants.
- `start_download(url: String, destination: Option<String>, connections: Option<usize>)`: Creates and spawns a download task.
- `pause_download(job_id: String)`: Pauses an active download and commits buffers to disk.
- `resume_download(job_id: String, refreshed_url: Option<String>)`: Resumes a paused or interrupted download.
- `cancel_download(job_id: String)`: Cancels download and removes partial files.
- `get_jobs()`: Queries all jobs from SQLite WAL.
- `get_engine_stats()`: Queries live throughput, active connections, CPU, memory, and disk IO.
- `run_diagnostics()`: Runs the doctor suite and returns the diagnostic report.
- `get_settings()` and `save_settings(settings: Settings)`: Reads and persists desktop preferences in JSON.

### Asynchronous event stream:
- `download://progress`: Pushes `ProgressSnapshot` updates from Tokio to the frontend every 250 milliseconds during active transfers.
- `download://completed`: Notifies the UI when a job finishes and passes SHA-256 verification.
- `download://error`: Pushes failure classifications and recovery notifications.

---

## 5. Visual design system specifications

The visual identity follows the four-screen concept reference:

- Color palette:
  - Background: Warm white / ivory (`#FAF7F5`).
  - Card background: Pure white (`#FFFFFF`) with subtle border (`#F0EAE6`) and soft shadow (`0 2px 8px rgba(0,0,0,0.03)`).
  - Primary accent: Soft pink (`#F472B6` / `#FB7185`).
  - Active navigation pill: Pale pink (`#FFEFF4`).
  - Text: Near-black primary (`#1F1A24`), muted secondary (`#6B6375`).
  - Success: Soft green (`#10B981` / `#D1FAE5`).
- Typography: System clean sans-serif (Segoe UI, Inter, -apple-system).
- Mascot integration:
  - Overview hero: `images/Flying Otter Mascot with Flowing Scarf.png`.
  - Downloads: `images/Focused Otter with Laptop and Coffee.png`.
  - Activity: `images/Cheerful Otter Popping Out of Pink Box.png`.
  - Settings: `images/Sleeping Otter with Pink Paw Pillow.png`.

---

## 6. Risks and compatibility requirements

1. Non-breaking CLI compatibility:
   - The CLI crate (`crates/cli`) and GUI crate (`crates/gui`) share the same `crates/engine` and `crates/types` code.
   - The shared SQLite database (`manifest.db`) maintains identical schemas and WAL settings across both interfaces.
2. Security boundaries:
   - Downloaded executables are never executed automatically.
   - SSRF checks prevent queries to private IP ranges.
   - Path validation prevents writes outside user-selected download directories.
3. Resource footprint:
   - Desktop idle memory consumption stays below 60 MB RAM.
   - UI thread does not block during high-throughput multi-stream disk writing.
