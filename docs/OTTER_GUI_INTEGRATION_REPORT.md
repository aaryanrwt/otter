# Otter GUI integration report

Document version: 1.0.0
Engine version: 0.1.0
Author: aaryanrwt

## 1. Engine and GUI integration layer

The Otter desktop application is engineered directly on top of the production Rust core crates rather than calling external CLI subprocesses or mocking engine responses.

```
+--------------------------------------------------------------+
|                     GUI Runtime Layer                        |
|                                                              |
|   crates/gui                                                 |
|   +------------------------------------------------------+   |
|   | AppState                                             |   |
|   |  - manager: Arc<DownloadManager>                     |   |
|   |  - manifest: Arc<ManifestStore>                      |   |
|   |  - cancellation_tokens: Arc<Mutex<HashMap<...>>>     |   |
|   |  - settings: Arc<Mutex<GuiSettings>>                 |   |
|   +------------------------------------------------------+   |
|                            |                                 |
|            +---------------+---------------+                 |
|            |                               |                 |
|            v                               v                 |
|   crates/engine                   crates/types               |
|   - probe::probe_url              - JobRecord                |
|   - orchestrator::DownloadManager - ProgressSnapshot         |
|   - manifest::ManifestStore       - DownloadStatus           |
|   - doctor::DoctorEngine          - RemoteFileInfo           |
+--------------------------------------------------------------+
```

---

## 2. Shared persistence contract (SQLite WAL)

Both the CLI (`otter.exe`) and the desktop application (`otter-gui.exe`) share the exact same SQLite WAL database:
- Path: `%LOCALAPPDATA%\otter\manifest.db`
- Journal Mode: `WAL` (Write-Ahead Logging)
- Synchronous setting: `NORMAL` (crash-resilient without disk stalls)

Because SQLite WAL supports multiple concurrent readers and a single concurrent writer without blocking reads, tasks started in the CLI immediately reflect in the GUI, and downloads initiated in the GUI are immediately inspectable using `otter list` or `otter resume` from the command line.

---

## 3. Real-time telemetry pipeline

Progress snapshots flow asynchronously from the active download streams to the webview:

1. Dynamic Range Workers calculate chunk throughput and disk write completion.
2. `DownloadManager` broadcasts snapshots over a `tokio::sync::broadcast::channel`.
3. The GUI background listener in `crates/gui/src/main.rs` receives the snapshot.
4. Active progress records update in memory (`active_snapshots`).
5. A JSON payload is dispatched to the WebView2 runtime using `evaluate_script("window.onOtterEvent(...)")`.
6. `app.js` updates the DOM dynamically without page reloads or layout thrashing.

---

## 4. Hardware introspection integration

To adhere strictly to genuine system telemetry (no hardcoded device names or mock memory sizes), the GUI integrates directly with the Win32 subsystem:
- Device Name: Querying `std::env::var("COMPUTERNAME")` directly from the OS environment.
- Logical CPU Cores: Introspected through `std::thread::available_parallelism()`.
- System Memory: Read via Win32 `GlobalMemoryStatusEx(&mut mem)` from `kernel32.dll`.
- Available Storage: Dynamically measured via `GetDiskFreeSpaceExW` through `engine::disk::get_available_disk_space()`.
