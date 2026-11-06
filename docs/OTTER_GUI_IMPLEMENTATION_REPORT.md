# Otter GUI implementation report

Document version: 1.0.0
Engine version: 0.1.0
Target platform: Windows 10 / Windows 11 (x86_64)
Author: aaryanrwt

## 1. Executive overview

The Otter desktop application provides a native Windows graphical user interface for the high performance Otter download engine. Rather than introducing an interpreted JavaScript runtime or heavy Electron container, Otter GUI uses Tao and Wry to run natively against the operating system's built-in Microsoft Edge WebView2 runtime.

The user interface matches the reference desktop specifications:
- Warm ivory and cloud canvas palette with pastel accents
- Four bespoke otter mascot states reflecting engine activities (flying, focused with laptop and coffee, cheerful in box, sleeping on paw pillow)
- Six structured navigation views (Overview, Downloads, Activity, Library, Engine telemetry, Settings)
- Universal URL detection modal supporting video streams, Hugging Face repositories, Ollama model tags, game stores, and direct files
- Direct bindings into the Rust core engine and SQLite WAL manifest database

---

## 2. Desktop architecture and technology stack

```
+-------------------------------------------------------------+
|                 Windows Desktop (Tao Window)                |
|  +-------------------------------------------------------+  |
|  |             Microsoft Edge WebView2 (Wry)             |  |
|  |  +-------------------------------------------------+  |  |
|  |  |  HTML5 / Modern CSS / Vanilla JS (No Framework)  |  |  |
|  |  |  otter:// Protocol Handler                      |  |  |
|  |  +-------------------------------------------------+  |  |
|  +---------------------------|---------------------------+  |
|                              | IPC (JSON messages)          |
|  +---------------------------v---------------------------+  |
|  |                     crates/gui (Rust)                 |  |
|  |  - UrlDetector (Universal URL parser & SSRF guard)    |  |
|  |  - GuiSettings (Persistent JSON in AppData)           |  |
|  |  - Win32 Hardware Introspection (RAM, Cores, Disk)   |  |
|  +---------------------------|---------------------------+  |
|                              | Direct Rust crate calls      |
|  +---------------------------v---------------------------+  |
|  |                     crates/engine                     |  |
|  |  - DownloadManager (HTTP Range & AIMD Controller)     |  |
|  |  - PositionedDiskWriter & Win32 SetFileValidData      |  |
|  |  - ManifestStore (SQLite in WAL mode)                 |  |
|  +-------------------------------------------------------+  |
+-------------------------------------------------------------+
```

### Key architectural advantages
1. Binary footprint: The complete GUI binary compiles directly into an optimized executable without bundling Chromium or Node.js.
2. Memory efficiency: Uses approximately 35 MB to 60 MB of RAM during multi-stream downloads, leaving host resources available for large machine learning workloads.
3. Protocol security: Assets and images are served internally through an embedded `otter://` virtual schema, preventing CORS issues and disallowing remote asset injection.
4. Thread safety: The user interface runs on an event loop thread while heavy download tasks and SQLite transactions run on a multi-threaded Tokio runtime.

---

## 3. View implementations

### 3.1 Overview screen
- Welcome banner with live user identity from the Windows environment.
- Summary cards: Active downloads, completed downloads, aggregate network speed, and total downloaded volume.
- Active tasks list with live animated progress bars and real-time MB/s telemetry.
- Hero mascot: Flying Otter with Flowing Scarf rendered on top-right.

### 3.2 Downloads screen
- Complete task manager listing all downloads recorded in SQLite.
- Filter pills: All, Active, Paused, Completed, Failed.
- Live search bar filtering by file name or source URL.
- Task controls: Pause, Resume, Cancel, and Delete (with optional physical file wipe).
- Mascot: Focused Otter with Laptop and Coffee.

### 3.3 Activity screen
- Application and ecosystem activity monitor.
- Live stream detection toggle.
- Activity breakdown cards for Chrome, Steam, Epic Games, Hugging Face, and GitHub.
- Mascot: Cheerful Otter Popping Out of Pink Box.

### 3.4 Library screen
- Categorized view of verified local assets:
  - Models (.gguf, .safetensors, .bin, .onnx)
  - Datasets (.parquet, .arrow, .jsonl, .csv)
  - Videos (.mp4, .mkv, .webm)
  - Archives (.zip, .tar.gz, .7z)
  - Documents (.pdf, .epub, .txt)
- One-click actions to open files directly or show in Windows Explorer.

### 3.5 Engine and diagnostics screen
- Real-time internal telemetry from the Rust orchestrator:
  - AIMD congestion window controller state
  - Current active HTTP TCP connection count
  - SQLite WAL status and checkpoint intervals
  - Windows file system preallocation state (`SetFileValidData`)
- System hardware audit showing actual host name, CPU cores, physical RAM, and partition capacity.
- Diagnostic suite execution running network, DNS, and disk checks directly from `engine::doctor`.

### 3.6 Settings screen
- Two-column management panel for General, Detection, Engine, Storage, Network, and Appearance.
- Default download folder configuration (defaults to `D:\Downloads\Otter` when Drive D: exists, or user profile downloads).
- Concurrency and bandwidth limits.
- Mascot: Sleeping Otter with Pink Paw Pillow.

---

## 4. Universal URL ingestion workflow

1. User clicks "+ Add Download" or presses hotkey to open modal.
2. Input field accepts raw URL string.
3. As the user types or pastes, `UrlDetector::detect()` validates:
   - Scheme verification (HTTP/HTTPS only).
   - SSRF protection (rejects localhost, RFC 1918 private subnets, link-local addresses).
   - Platform matching and metadata extraction (Ollama model tag, Hugging Face model/dataset, YouTube, Steam app, Epic Games, direct links).
4. Detected information is rendered immediately with platform badges and suggested filenames.
5. Clicking "Start Download" dispatches a probe to the remote host, reserves SQLite records, and launches parallel range streams through `DownloadManager`.
