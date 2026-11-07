# OTTER v0.1.0 — Desktop GUI Implementation Plan
**Document Version:** 1.0.0  
**Status:** Completed & Verified  
**Engine:** Rust Universal Engine (`crates/engine`, `crates/types`, `crates/gui`)  
**AI Workflow:** Antigravity + Local Ollama (`qwen2.5-coder:7b`)

---

## 1. Executive Summary & Goals
The objective of OTTER v0.1.0 Desktop GUI is to provide a clean, minimal, warm-toned desktop interface powered directly by OTTER's native multi-connection Rust engine. The GUI is not a mockup or prototype: every progress bar, speed metric, stream count, and persistent state reflects genuine backend execution.

---

## 2. Component Implementation Breakdown

### 2.1 Core Rust Engine Integration (`crates/gui/src/main.rs`)
- **Native Tao / Wry Windowing:** Single-instance Win32 mutex (`Local\OtterUniversalDownloadManager_Mutex`) preventing WebView2 multi-process resource locks (`HRESULT 0x800700AA`).
- **Asynchronous IPC Bridge:** Tao `EventLoopProxy<UserEvent>` coupled with Tokio multi-threaded runtime.
- **State Serialization (`crates/gui/src/state.rs`):** Atomic configuration persistence at `%LOCALAPPDATA%\otter\gui_settings.json` with dynamic path fallbacks to `%USERPROFILE%\Downloads\Otter`.
- **Manifest Engine (`crates/engine/src/manifest.rs`):** SQLite persistence (`manifest.db`) tracking active, queued, paused, completed, and failed jobs.

### 2.2 Universal Media & Direct Download Engine (`crates/gui/scripts/media_engine.py`)
- **Dynamic Resolution Discovery:** Direct probing with `yt-dlp` extracting only actual available stream resolutions (from 4K 2160p down to 144p).
- **Fast Remuxing:** Container merge using `ffmpeg` with `merge_output_format: 'mp4'` avoiding slow transcode loops.
- **Audio Extraction:** High-bitrate MP3 and M4A extraction with `FFmpegExtractAudio`.
- **Pure JSON Inter-Process Protocol:** Clean stdout formatting with `'noprogress': True` ensuring non-corrupt deserialization in Rust.

### 2.3 UI / UX System (`crates/gui/ui/`)
- **Visual Design System:** Minimal warm ivory canvas (`#FFFDFB`), crisp card containers (`#FFFFFF`), subtle borders (`rgba(0,0,0,0.06)`), and Otter pink accents (`#F472B6`, `#BE185D`).
- **5-Tab Sidebar:** Overview, Downloads, Activity, Library, Settings (Engine tab completely removed from main navigation).
- **Authentic Logo Assets:** 100% genuine vector and bitmap logos for YouTube, Google Chrome, Steam, Epic Games, Hugging Face, GitHub, Instagram, and Pinterest.
- **Dynamic User Onboarding:** Name modal on first run, updating "Welcome back, {User}" without hardcoded user identities.

---

## 3. Concurrency & Scaling Policy
- **Maximum Simultaneous Download Jobs:** 16 (enforced globally across GUI queues).
- **Streams Per Job:** 1–16 dynamic streams governed by file size and AIMD congestion control.
- **Cancellation & Deletion:** Immediate asynchronous process termination, token cancellation, partial file removal, and SQLite record deletion.

---

## 4. Implementation Schedule & Milestones
- **Milestone 1 (Architecture & Navigation):** 5-tab minimal layout, single-instance mutex, and authentic asset integration. *(Status: PASS)*
- **Milestone 2 (Universal URL & Media Engine):** yt-dlp + ffmpeg dynamic probe, resolution filters, and direct download engine. *(Status: PASS)*
- **Milestone 3 (Activity & Process Telemetry):** Real Windows process detection and honest status indicators. *(Status: PASS)*
- **Milestone 4 (Settings & Persistence):** Dynamic user Downloads path resolution, autostart registry, and theme persistence. *(Status: PASS)*
- **Milestone 5 (Verification & Benchmarking):** Real file tests, memory analysis, and crash recovery. *(Status: PASS)*
