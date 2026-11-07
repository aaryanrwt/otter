# Otter Settings Architecture Audit

Document version: 1.0.0
Application: Otter v0.1.0
Author: aaryanrwt

## 1. Existing settings implementation

Otter currently persists desktop GUI configuration in:
- File path: `%LOCALAPPDATA%\otter\gui_settings.json`
- Storage format: Pretty-printed JSON (`serde_json`)
- In-memory representation: `Arc<tokio::sync::Mutex<GuiSettings>>` in `crates/gui/src/state.rs`
- Thread-safe synchronization: In-memory mutex synchronized with disk persistence on every update.

## 2. Existing settings status

| Setting | Current implementation | Verification | Runtime effect |
| :--- | :--- | :--- | :--- |
| User display name | `user_name: String` in `gui_settings.json` | Verified via onboarding pop-up | Updates greeting immediately |
| Default download location | `default_download_dir: String` | Verified with D:\Downloads\Otter fallback | Applied to future jobs |
| Concurrent downloads | `concurrent_downloads: usize` (default 5) | Saved in JSON | Configures orchestrator limits |
| Bandwidth limit | `bandwidth_limit_mb_s: Option<f64>` | Saved in JSON | Engine bandwidth rate limiter |
| Start with Windows | `start_with_windows: bool` | Saved in JSON; needs HKCU Run key | Registry auto-start on Windows |
| Minimize to system tray | `minimize_to_tray: bool` | Saved in JSON; needs window hide on close | Background tray execution |
| Live detection | `live_detection: bool` | Saved in JSON | Toggles dynamic discovery |
| Theme | `theme: String` ("light" / "dark") | Verified with CSS variables | Applies immediately |
| Language | `language: String` ("English") | Saved in JSON | Display language |

## 3. Configuration persistence architecture

1. Authoritative store: `%LOCALAPPDATA%\otter\gui_settings.json`.
2. Lifecycle:
   - On startup: Loaded from JSON; if missing, populated with sensible defaults.
   - On change: UI sends `save_settings` IPC to Rust backend.
   - Backend atomic write: Serializes to temporary file, then renames to avoid partial write corruption.
   - In-memory state updated and broadcast back to webview via `onOtterEvent({ type: 'state_update' })`.

## 4. Settings update classifications

- Immediate update (no restart required):
  - User name, Theme, Default download directory, Concurrent downloads limit, Bandwidth rate limit, Live detection toggle, Start with Windows (registry key).
- Safe download boundaries:
  - Changing default download directory only affects future downloads; active downloads preserve their destination path.
  - Resetting settings preserves downloaded files, manifests, and benchmark records.
