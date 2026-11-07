# OTTER v0.1.0 — Settings Feature Matrix

| Feature Key | UI Control | Storage Target | Backend Action | Immediate / Restart |
| :--- | :--- | :--- | :--- | :--- |
| `start_with_windows` | Pink Toggle | `gui_settings.json` | `reg add HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v Otter` | Immediate |
| `minimize_to_tray` | Pink Toggle | `gui_settings.json` | Updates GUI window close intercept handler | Immediate |
| `language` | Pill Select (`English ⌵`) | `gui_settings.json` | Persisted for localization bundles | Immediate |
| `default_download_dir` | Label + Pill Button `Change` | `gui_settings.json` | Opens native Win32 `FolderBrowserDialog` | Immediate |
| `concurrent_downloads` | Pill Select (`5 ⌵`) | `gui_settings.json` | Updates job concurrency queue ceiling | Immediate |
| `bandwidth_limit_mb_s` | Pill Select (`No limit ⌵`) | `gui_settings.json` | Throttles token bucket in stream writer | Immediate |
| `live_detection` | Pink Toggle | `gui_settings.json` | Enables clipboard watcher loop | Immediate |
| `show_detection_preview` | Pink Toggle | `gui_settings.json` | Enables detection result card in ingestion modal | Immediate |
| `aimd_adaptive_concurrency` | Pink Toggle | `gui_settings.json` | Enables engine AIMD dynamic stream scaling | Immediate |
| `auto_verify_sha256` | Pink Toggle | `gui_settings.json` | Triggers post-download cryptographic hashing | Immediate |
| `max_connections_per_host` | Pill Select | `gui_settings.json` | Updates engine per-host semaphore ceiling | Immediate |
| `theme` | Pill Select (`Warm Ivory ⌵`) | `gui_settings.json` | Toggles DOM `data-theme` attribute | Immediate |
| `notify_on_completed` | Pink Toggle | `gui_settings.json` | Dispatches native Win32 notification toasts | Immediate |
| `notify_on_failed` | Pink Toggle | `gui_settings.json` | Dispatches native error alerts on network reset | Immediate |
| `open_settings_dir` | Outline Pill Button | N/A | Opens Windows Explorer to `%LOCALAPPDATA%\otter` | Immediate |
| `reset_settings` | Outline Pill Button | `gui_settings.json` | Restores default configuration safely | Immediate |
