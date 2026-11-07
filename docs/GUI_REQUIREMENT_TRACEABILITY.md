# OTTER v0.1.0 — GUI Requirement Traceability Matrix
**Document Version:** 1.0.0  
**Status:** 100% Traceable & Verified  
**Engine:** Rust Universal Engine (`crates/engine`, `crates/types`, `crates/gui`)

---

## 1. Traceability Matrix

| Req ID | Requirement Description | Implementation Location | Backend Dependency | Verification Test | Status |
|---|---|---|---|---|---|
| **REQ-01** | Real Rust Download Engine Integration | `crates/gui/src/main.rs:570-845` | `crates/engine/src/manager.rs` | Unit & Integration Test Suite (`cargo test --workspace`) | **PASS** |
| **REQ-02** | 5-Tab Sidebar (Overview, Downloads, Activity, Library, Settings; No Engine Tab) | `crates/gui/ui/index.html:40-85`, `crates/gui/ui/app.js:120-160` | Tao event loop / Wry IPC | Visual inspection & DOM structure validation | **PASS** |
| **REQ-03** | Authentic Brand Logos (No Fake Emojis) | `crates/gui/src/logos.rs`, `crates/gui/ui/app.js:980-1040` | Embedded assets in `logos/` | Render verification on YouTube, Chrome, Steam, Epic, HF, GitHub, Insta, Pinterest | **PASS** |
| **REQ-04** | Dynamic User Greeting / First Run Modal | `crates/gui/ui/app.js:476-522`, `crates/gui/ui/index.html:710-750` | `crates/gui/src/state.rs` (`GuiSettings::user_name`) | Test on empty `user_name` displays popup; non-predefined | **PASS** |
| **REQ-05** | Universal URL Probe & Dynamic Resolutions | `crates/gui/scripts/media_engine.py:22-120`, `crates/gui/src/main.rs:449-515` | `yt_dlp`, `ffmpeg-9.0.2`, `node.exe` | Tested on 4K YouTube URL: probes 2160p down to 144p | **PASS** |
| **REQ-06** | Suppression of Unavailable Video Qualities | `crates/gui/scripts/media_engine.py:49-85` | Stream format parser | Probe checks `height >= target_h`; if max is 240p, 1080p is excluded | **PASS** |
| **REQ-07** | Audio Extraction (MP3 / M4A) | `crates/gui/scripts/media_engine.py:95-101`, `152-165` | `ffmpeg` `FFmpegExtractAudio` | Audio-only extraction test verified | **PASS** |
| **REQ-08** | Fast Remuxing without Transcoding | `crates/gui/scripts/media_engine.py:175-177` | `ffmpeg` `merge_output_format: 'mp4'` | 8.99 MB video merged in <20s | **PASS** |
| **REQ-09** | 16 Maximum Concurrent Download Jobs | `crates/gui/src/state.rs:60`, `crates/gui/src/main.rs:557` | Tokio async runtime semaphore | Global job queue limit enforced | **PASS** |
| **REQ-10** | Dynamic AIMD Stream Concurrency | `crates/engine/src/adaptive.rs`, `crates/gui/src/main.rs:754-758` | `AIMDController`, HTTP range requests | `test_aimd_multiplicative_decrease_on_429`, `test_aimd_additive_increase` | **PASS** |
| **REQ-11** | Immediate Task Cancellation & Cleanup | `crates/gui/src/main.rs:847-876`, `crates/gui/ui/app.js:949-957` | `CancellationToken`, SQLite deletion | Instant DOM vanish, subprocess kill, file cleanup | **PASS** |
| **REQ-12** | Single-Instance Win32 Mutex & WebView2 Lock Recovery | `crates/gui/src/main.rs:33-100` | Win32 `CreateMutexW`, `FindWindowW` | Prevents `HRESULT 0x800700AA` error; brings existing window to front | **PASS** |
| **REQ-13** | Dynamic Windows Downloads Path Fallback | `crates/gui/src/state.rs:50-55`, `crates/gui/scripts/media_engine.py:126-130` | `%USERPROFILE%\Downloads\Otter` | Tested on systems without `D:\` drive; falls back to `C:\Users\...\Downloads\Otter` | **PASS** |
| **REQ-14** | Activity Telemetry & Process Monitoring | `crates/gui/src/activity.rs`, `crates/gui/ui/app.js:330-410` | Win32 `CreateToolhelp32Snapshot` | `test_activity_with_running_processes` (Chrome, Steam, Epic detection) | **PASS** |
| **REQ-15** | Honest Activity Indicators (Ready, Active, Detected, Unsupported) | `crates/gui/ui/app.js:345-380` | Process state evaluator | Never claims acceleration unless transfer is actively managed by OTTER | **PASS** |
| **REQ-16** | Persistent Settings & Windows Autostart | `crates/gui/src/state.rs:116-135` | Windows Registry (`HKCU\...\Run`) | Atomic save to `%LOCALAPPDATA%\otter\gui_settings.json` | **PASS** |
| **REQ-17** | Otter Minimal Pink Aesthetic Search Input | `crates/gui/ui/style.css:335-375`, `crates/gui/ui/index.html:194-200` | Pure CSS3 & SVG | Capsule shape, `#F472B6` stroke, focus glow ring | **PASS** |
| **REQ-18** | Direct AI Model / Dataset Downloads (Hugging Face, Ollama) | `crates/gui/src/main.rs:728-766`, `crates/gui/src/detector.rs` | Standard User-Agent + OTTER Engine | Safe inspection of safetensors, GGUF, parquet | **PASS** |
