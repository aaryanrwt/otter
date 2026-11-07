# OTTER v0.1.0 — GUI Architecture & Technical Design
**Document Version:** 1.0.0  
**Status:** Completed  
**Engine:** Rust Universal Engine (`crates/engine`, `crates/types`, `crates/gui`)

---

## 1. System Architecture Overview

OTTER Desktop GUI is architected with clear boundaries between the native OS layer, the Rust download engine, and the webview interface:

```
┌─────────────────────────────────────────────────────────────┐
│                    OTTER Desktop Frontend                   │
│   (HTML5, Vanilla ES6 JavaScript, CSS3 Design Tokens)        │
│   - Overview Dashboard       - Downloads Manager            │
│   - Activity Telemetry       - Library (Persisted SQLite)   │
│   - Minimal Settings         - Universal URL Modal          │
└──────────────────────────────┬──────────────────────────────┘
                               │ IPC (JSON via wry::evaluate_script
                               │      & window.ipc.postMessage)
┌──────────────────────────────▼──────────────────────────────┐
│                    Rust GUI Host Runtime                    │
│   (Tao Windowing + Wry WebView2 + Tokio Multi-threaded RT)  │
│                                                             │
│   ┌─────────────────────┐       ┌───────────────────────┐   │
│   │ SingleInstanceGuard │       │  Activity & Detection │   │
│   │  (Win32 Mutex)      │       │  (Toolhelp32 Snapshot)│   │
│   └─────────────────────┘       └───────────────────────┘   │
│   ┌─────────────────────────────────────────────────────┐   │
│   │               State & Settings Manager              │   │
│   │       (Atomic JSON & Dynamic Path Fallbacks)        │   │
│   └─────────────────────────────────────────────────────┘   │
└──────────────────────────────┬──────────────────────────────┘
                               │
               ┌───────────────┴───────────────┐
               ▼                               ▼
┌──────────────────────────────┐ ┌────────────────────────────┐
│      OTTER Rust Engine       │ │   Media Stream Adapter     │
│   (`crates/engine`)          │ │ (`scripts/media_engine.py`)│
│  - Multi-connection HTTP     │ │ - yt-dlp + ffmpeg remux    │
│  - AIMD Concurrency Engine   │ │ - Dynamic resolution probe │
│  - Work-Stealing Allocator   │ │ - Audio extraction (MP3)   │
│  - Positioned Disk Writer    │ │ - Direct JSON stdout hook  │
│  - SHA-256 Verifier          │ └────────────────────────────┘
│  - SQLite Job Manifest       │
└──────────────────────────────┘
```

---

## 2. Component Directory Structure

```
crates/gui/
├── Cargo.toml
├── build.rs
├── scripts/
│   └── media_engine.py       # yt-dlp + ffmpeg extraction pipeline
├── src/
│   ├── activity.rs           # Windows process telemetry & activity monitor
│   ├── detector.rs           # Universal URL categorizer & security validator
│   ├── logos.rs              # Authentic brand logo asset manager
│   ├── main.rs               # Tao/Wry window manager, IPC event loop, Tokio RT
│   └── state.rs              # Settings persistence, path resolution, registry autostart
└── ui/
    ├── app.js                # Frontend controller, event dispatcher, modal manager
    ├── index.html            # 5-tab semantic view layout
    └── style.css             # Minimal warm ivory and pink design tokens
```

---

## 3. Communication & Concurrency Model

### 3.1 Asynchronous Event Loop
The GUI application runs a dual-loop pattern:
1. **OS UI Thread:** Powered by `tao::event_loop::EventLoop<UserEvent>`. Handles rendering, window resizing, and message interception.
2. **Tokio Async Worker Pool:** Handles network requests, download chunk scheduling, disk writing, and child process execution.

### 3.2 Single-Instance Mutex
To eliminate the `HRESULT(0x800700AA): The requested resource is in use` error caused by multiple processes competing for WebView2 user data folders, `SingleInstanceGuard` uses the Win32 API (`CreateMutexW`). If an instance already exists, `FindWindowW` restores and brings the existing window to the foreground, exiting the duplicate process cleanly.

### 3.3 State Synchronization
All download state resides in the backend SQLite manifest and active in-memory progress snapshots. The UI never maintains its own independent state: it renders the immutable snapshot pushed via `PushStateUpdate(String)` on every 500ms cycle or on state change events.
