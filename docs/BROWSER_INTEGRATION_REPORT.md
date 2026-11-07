# OTTER v0.1.0 — Browser Integration & Detection Report
**Document Version:** 1.0.0  
**Status:** Architecture Specified & Process Telemetry Implemented  
**Engine:** Rust Universal Engine (`crates/engine`, `crates/gui`)

---

## 1. Overview & Integration Philosophy

OTTER v0.1.0 provides honest detection and integration with desktop web browsers (Google Chrome, Microsoft Edge, Mozilla Firefox) following these strict principles:
- **No Spoofing:** A download is marked as "Managed by OTTER" only if OTTER is actively handling the HTTP connection and disk writing.
- **No Ghost Progress:** OTTER never simulates browser download speeds or fabricated progress bars.

---

## 2. Windows-Level Process & Activity Detection (`crates/gui/src/activity.rs`)

### 2.1 Process Snapshot Telemetry
Using the native Windows Toolhelp32 API (`CreateToolhelp32Snapshot`), OTTER scans for active browser instances (`chrome.exe`, `msedge.exe`, `firefox.exe`, `brave.exe`):
- If the browser process is running, its status in the Activity page transitions from `Standby` to `Ready` or `Active`.
- Process IDs and window handles are inspected to identify active user sessions.

### 2.2 Download Directory Monitoring
OTTER monitors the standard user Downloads directory (`%USERPROFILE%\Downloads`) using filesystem notification hooks (`ReadDirectoryChangesW`):
- Active temporary browser downloads (e.g. `.crdownload` for Chrome/Edge, `.part` for Firefox) are identified.
- Filename, size on disk, and timestamp are extracted.
- When the download completes and the extension changes to the final format, OTTER indexes the asset into the Library.

---

## 3. Native Messaging Host Architecture (Browser Extensions)

For full download interception and acceleration handoff:
1. **Manifest File (`otter_native_host.json`):**
   Registered in the Windows Registry at:
   `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.otter.downloadmanager`
   `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.otter.downloadmanager`
2. **Browser Extension (`chrome.downloads` API):**
   - Listens to `chrome.downloads.onCreated`.
   - When a download begins, the extension passes the URL, cookies, user-agent, and destination path to OTTER via native messaging stdin/stdout.
   - If the user enables "Ask before taking over browser downloads", OTTER displays a confirmation modal; if accepted, the browser download is cancelled and handed off to OTTER's high-speed engine.

---

## 4. Current Status & Known Constraints
- **Active Process Detection:** 100% Functional (tested in `test_activity_with_running_processes`).
- **Filesystem Detection:** 100% Functional (indexes active `.crdownload` and `.part` files).
- **Native Extension Manifest:** Provided for installation by users desiring automatic interception.
- **Protected Content:** Browser-managed extensions with DRM or one-time session tokens (e.g., Netflix, Spotify Web) cannot be intercepted and are honestly marked as `Unsupported`.
