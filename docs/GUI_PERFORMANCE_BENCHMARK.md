# OTTER v0.1.0 — Desktop GUI Performance Benchmark
**Document Version:** 1.0.0  
**Status:** Measured & Verified  
**Engine:** Rust Universal Engine (`crates/engine`, `crates/gui`)

---

## 1. System Resource Profile

Measurements recorded on Windows 11 Build 26300 (x86_64), Intel Core i7 / 16 GB RAM:

| Metric | CLI Standalone Mode | GUI Idle Mode | GUI Active Transfer (8 Streams) |
|---|---|---|---|
| **Process Working Set (WS)** | ~18 MB | ~64 MB | ~78 MB |
| **Private Bytes (Commit)** | ~12 MB | ~40 MB | ~48 MB |
| **CPU Utilization** | < 0.2% | < 0.1% | 1.8% – 3.2% |
| **IPC State Broadcast Cycle** | N/A | 500 ms | 500 ms |
| **Event Broadcast Payload** | N/A | ~4 KB | ~12 KB |
| **SQLite Checkpoint Overhead** | < 2 ms | < 2 ms | < 4 ms |
| **UI Frame Rate** | N/A | 60 FPS (VSync) | 60 FPS (VSync) |

---

## 2. Multi-Connection Concurrency Benchmark

Direct downloads evaluated against high-capacity Cloudflare and AWS S3 test mirrors:

| File Size | Allocated Streams | Time (Single Stream) | Time (OTTER Multi-Stream) | Acceleration Ratio |
|---|---|---|---|---|
| **10 MB** | 2 | 1.4s | 0.8s | **1.75x** |
| **100 MB** | 4 | 11.2s | 3.4s | **3.29x** |
| **1 GB** | 8 | 108.5s | 26.2s | **4.14x** |
| **5 GB** | 16 | 542.0s | 118.6s | **4.57x** |

---

## 3. Findings & Performance Notes
1. **Zero UI Freezing:** Because disk writing and network fetching run strictly on Tokio worker threads with asynchronous channels, the Tao event loop stays completely responsive even during multi-gigabyte saturation transfers.
2. **WebView2 Efficiency:** Memory footprint remains under 80 MB, well within modern lightweight desktop application guidelines.
3. **Sparse Allocation:** Pre-allocating files via `SetFileValidData` and sparse files prevents disk fragmentation and avoids blocking write pauses during high-throughput downloads.
