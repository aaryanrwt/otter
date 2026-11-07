# OTTER v0.1.0 — Chrome Browser Integration & Download Handoff Architecture Audit

**Date:** 2026-11-07  
**Engine Version:** `OTTER 0.1.0`  
**Target Environment:** Windows 11 `x86_64-pc-windows-msvc`, Google Chrome (Manifest V3), NTFS Drive `D:\`  
**Test Directory:** `D:\OTTER_CHROME_BENCHMARK\`  

---

## 1. Executive Summary & Existing Codebase Audit

An exhaustive audit of the existing OTTER repository (`crates/gui`, `crates/engine`, `crates/cli`, `crates/types`, `crates/harness`) was conducted to evaluate browser detection capabilities prior to modifying code.

### Findings on Current Code:
1. **Passive Filesystem Polling Only:** In `crates/gui/src/activity.rs` (`refresh_live_activity`), OTTER checks if `chrome.exe` exists in `running_processes` and scans the user's `Downloads` directory for files matching `*.crdownload`.
2. **Missing Source URL & Metadata:** In-flight `.crdownload` files (`Unconfirmed xxx.crdownload`) do not have NTFS `Zone.Identifier` ADS written until completion or rename. As a result, source URL, MIME type, true target filename, and expected remote size are completely unavailable from disk polling alone.
3. **Hardcoded Mock Telemetry:** Lines 314 and 342 of `crates/gui/src/activity.rs` contain a hardcoded string `"92.4 MB/s"` and an artificial `50.0%` progress calculation for detected `.crdownload` files, violating the rule against fabricated UI state.
4. **Zero Interception or Handoff:** There is no Chrome Extension, no Native Messaging Host registered in the Windows Registry (`HKCU\Software\Google\Chrome\NativeMessagingHosts`), no download handoff protocol, and no cancellation/pause coordination with Chrome.
5. **Double-Download Vulnerability:** If OTTER were to independently download a URL detected in Chrome without coordinating, both Chrome and OTTER would download the same file simultaneously, wasting user bandwidth.

---

## 2. Answers to the 10 Core Audit Questions

| # | Question | Current Status | Technical Audit Details |
| :-: | :--- | :---: | :--- |
| **1** | Can OTTER detect a new Chrome download automatically? | **PARTIAL** | Only passively detects `.crdownload` files after Chrome creates them on disk. Does not receive real-time download-initiated push notifications from the browser. |
| **2** | Can it identify the source URL without exposing credentials? | **FAIL** | Passive disk polling cannot read the URL of an active in-flight download. When NTFS ADS exists post-download, query parameters with session tokens can be exposed unless explicitly redacted. |
| **3** | Can it obtain the filename and actual expected size? | **FAIL** | Filename appears as `Unconfirmed <random>.crdownload` during transit. Total remote size (`Content-Length`) is unknown to the filesystem until complete. |
| **4** | Can it identify MIME type and file extension? | **FAIL** | Filesystem scan has no HTTP response headers (`Content-Type`, `Content-Disposition`) and only sees the temporary `.crdownload` extension. |
| **5** | Can it distinguish a Chrome download from an OTTER-managed download? | **PASS** | OTTER-managed jobs are stored in SQLite (`manifest.db`) and internal `SharedAppState`. Chrome jobs are segregated into the Chrome activity card. |
| **6** | Can it detect when Chrome pauses, fails, or completes a download? | **FAIL** | When Chrome pauses, `.crdownload` remains stagnant on disk; when it completes, Chrome renames the file, causing OTTER's filesystem scan to simply drop it from active list. |
| **7** | Can OTTER safely take control of a supported download? | **NOT IMPLEMENTED** | No handoff IPC exists. OTTER cannot signal Chrome to pause/cancel its download, nor transfer byte ranges safely. |
| **8** | Does the current integration actually accelerate transfers, or merely monitor Chrome? | **FAIL** | Current code merely monitors Chrome files passively and displays hardcoded `92.4 MB/s` telemetry; it provides zero stream acceleration. |
| **9** | Can OTTER recover after Chrome closes? | **FAIL** | Because OTTER has no URL, no range manifest, and no resume context for raw `.crdownload` files, closing Chrome leaves the download stranded. |
| **10** | Can OTTER recover after Windows restarts? | **NOT IMPLEMENTED** | For Chrome downloads, OTTER retains zero persistent records in its SQLite database. |

---

## 3. Requirement Traceability Matrix (Phase 0)

| Requirement ID | Specification | Status | Gap & Required Architecture |
| :--- | :--- | :---: | :--- |
| **`REQ-BRW-001`** | Real-time Chrome download detection | **NOT IMPLEMENTED** | Chrome Extension (Manifest V3) with `chrome.downloads.onCreated` and `chrome.downloads.onDeterminingFilename`. |
| **`REQ-BRW-002`** | Native Messaging Host / Local Bridge | **NOT IMPLEMENTED** | Register Windows Native Messaging Host (`com.otter.download_engine`) + Loopback IPC Server (`127.0.0.1:45771`) in `crates/gui`. |
| **`REQ-BRW-003`** | Metadata Capture & Redaction | **NOT IMPLEMENTED** | Capture `url`, `finalUrl`, `filename`, `mime`, `totalBytes`, `dangerType`; redact query authentication parameters (`?sig=`, `?token=`, `?expires=`) in logs. |
| **`REQ-BRW-004`** | Range Support & Acceleration Probe | **PARTIAL** | Engine `probe_url` exists in `crates/engine`, but is not hooked to incoming browser events. Must send `HEAD` / `GET Range: bytes=0-0` to verify `Accept-Ranges: bytes`. |
| **`REQ-BRW-005`** | Safe Handoff Protocol | **NOT IMPLEMENTED** | Must confirm eligibility, pause/cancel Chrome download via `chrome.downloads.pause(id)`, preallocate on Drive `D:`, and transition to OTTER engine without double-downloading. |
| **`REQ-BRW-006`** | Dynamic AIMD Stream Adaptation | **PASS** | `AdaptiveGovernor` exists in `crates/engine/src/adaptive.rs` with `1..=64` streams. Needs telemetry logging for adaptation sequences (e.g. `16 -> 32 -> 44 -> 32 -> 16 -> 10 -> 16 -> 4`). |
| **`REQ-BRW-007`** | One Logical File in GUI | **PASS** | `crates/gui` supports multi-stream aggregation with expandable drawers. |
| **`REQ-BRW-008`** | Real-time Telemetry (No Mock Data) | **FAIL** | Remove hardcoded `"92.4 MB/s"` in `activity.rs` and compute true bytes/second from live socket read deltas. |
| **`REQ-BRW-009`** | Interruption & Restart Durability | **PASS (Engine) / FAIL (Browser)** | Native OTTER jobs persist to SQLite `manifest.db` with 256 KB sub-range checkpoints; browser handoff jobs must register as persistent SQLite jobs. |
| **`REQ-BRW-010`** | Expired Signed-URL Detection | **PASS** | Handled via `HTTP 403` / `HTTP 410` error classification with clear explanation that browser session refresh is required for protected one-time links. |

---

## 4. Architectural Plan for Phases 1–9

```
+-------------------------------------------------------------------------------+
|                               Google Chrome                                   |
|  +--------------------+       +--------------------------------------------+  |
|  |  User clicks link  | ----> | Chrome Extension (Manifest V3)             |  |
|  +--------------------+       | - chrome.downloads.onCreated               |  |
|                               | - chrome.downloads.onDeterminingFilename   |  |
|                               +---------------------+----------------------+  |
+-----------------------------------------------------|-------------------------+
                                                      |
                   Native Messaging (stdin/stdout)    | Loopback IPC
                   or Native Bridge WebSocket / HTTP  | (127.0.0.1:45771)
                                                      v
+-------------------------------------------------------------------------------+
|                            OTTER Desktop Application                          |
|  +-------------------------------------------------------------------------+  |
|  | Browser Integration Bridge (crates/gui/src/browser_bridge.rs)           |  |
|  | - Ingests download metadata (filename, url, totalBytes, mime)           |  |
|  | - Probes server capabilities (Accept-Ranges, Content-Length, ETag)      |  |
|  | - Determines eligibility:                                               |  |
|  |     * Range supported + Content-Length > 0  --> ELIGIBLE FOR OTTER      |  |
|  |     * No Range or Streaming                 --> BROWSER NATIVE ONLY     |  |
|  +--------------------+------------------------------------+---------------+  |
|                       |                                    |                  |
|       Safe Handoff    | If Eligible                        | Telemetry        |
|       Signal          v                                    v                  |
|  +--------------------+-------+               +------------+---------------+  |
|  | Pause/Cancel Chrome Task   |               | Activity View (app.js)     |  |
|  | Hand off to OTTER Engine   |               | - Chrome Brand Logo        |  |
|  +--------------------+-------+               | - True Bps & Progress      |  |
|                       |                       | - Accelerate / Handoff Btn |  |
|                       v                       +----------------------------+  |
|  +--------------------+----------------------------------------------------+  |
|  | OTTER Rust Engine (crates/engine)                                       |  |
|  | - Multi-Stream Range Workers (D:\OTTER_CHROME_BENCHMARK\)               |  |
|  | - AIMD Dynamic Worker Adaptation (1..=64)                               |  |
|  | - SQLite WAL Checkpointing (0-byte retransmission on crash)             |  |
|  | - Streaming SHA-256 Verification & Final Atomic Assembly                |  |
|  +-------------------------------------------------------------------------+  |
+-------------------------------------------------------------------------------+
```
