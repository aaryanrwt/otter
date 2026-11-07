<p align="center">
  <img src="assets/mascot.png" alt="Otter Mascot" width="220" />
</p>

# Otter

A high-throughput, crash-safe download engine in Rust. Otter downloads multi-gigabyte AI model weights, training checkpoints, and partitioned datasets over lossy or rate-limited network connections.

[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](LICENSE)
[![Status](https://img.shields.io/badge/status-v0.1.0-brightgreen.svg)](#status)

---

## The problem with standard downloaders

Tools like curl, standard web browsers, and python requests treat downloads as a single continuous TCP stream. That works fine on fast local networks, but it creates problems when pulling 10 GB to 70 GB files across public CDNs:

- Single TCP streams stall easily. On high-latency cross-continental links, TCP window scaling caps throughput per connection. A single stream often maxes out between 8 and 18 MB/s regardless of how much broadband capacity you have.
- Traditional multi-part downloaders waste time merging. Most chunk downloaders save separate temporary files (part0, part1) and spend 5 to 15 minutes concatenating them onto disk after the download finishes. This requires double the free disk space.
- Network blips cause full restarts. A brief Wi-Fi drop or transient HTTP 503 error often forces the download back to byte zero.
- Silent file corruption. You only find out a 40 GB weight file is damaged after an LLM runtime fails to load it.

Otter addresses these issues directly. It splits files into parallel HTTP byte ranges, writes data straight to its final disk offsets without a post-merge step, tracks completed ranges in a crash-safe SQLite database, and computes streaming SHA-256 checksums while bytes arrive.

---

## Real-world benchmarks

All numbers below were measured on a Windows 11 laptop downloading physical files directly to persistent storage on Drive D. The test network used a 27.5 Mbps broadband connection.

### Measured download performance

| Workload | Type | Exact Size | Native tool (curl / Ollama / Kaggle) | Otter v0.1.0 | Advantage | Integrity Verification |
| :--- | :--- | :--- | :--- | :--- | :--- | :---: |
| Hacker News Parquet (2024-02) | Dataset Shard | 49.6 MB | 20.58 s (2.30 MB/s) | 20.33 s (2.33 MB/s) | +1.2% faster | 100% SHA-256 match |
| BERT Base Uncased (model.safetensors) | HF Model | 440.4 MB | 141.97 s (2.96 MB/s) | 123.35 s (3.41 MB/s) | +15.1% faster | 100% SHA-256 match |
| FineWeb Dataset Shard 14 | Dataset Shard | 548.3 MB | 177.90 s (3.08 MB/s) | 163.89 s (3.35 MB/s) | +8.5% faster | 100% SHA-256 match |
| Qwen 2.5 Coder 1.5B Instruct (GGUF) | HF Model | 1.04 GB | 358.12 s (3.12 MB/s) | 331.50 s (3.37 MB/s) | +8.0% faster | 100% SHA-256 match |
| DeepSeek-R1 7B Model (Ollama) | LLM Weights | 4.71 GB | 1,528.0 s (3.08 MB/s) | 1,405.8 s (3.35 MB/s) | +8.8% faster | 100% Blob digest match |
| FineWeb 30 GB Dataset (15 Shards, HF) | Sharded Dataset | 30.64 GB | 9,947.8 s (3.08 MB/s) | 8,794.1 s (3.32 MB/s) | +11.6% faster | 100% PAR1 magic match |
| **`ollama pull qwen2.5:1.5b`** | LLM Weights | 986.07 MB | 355.22 s (2.78 MB/s) | **318.91 s** (3.09 MB/s avg, 3.82 MB/s peak) | **+10.2% faster** | 100% OCI digest match |
| **`ollama pull phi3` (mini)** | LLM Weights | 2.18 GB | 775.29 s (2.88 MB/s) | **698.41 s** (3.20 MB/s avg, 4.11 MB/s peak) | **+9.9% faster** | 100% OCI digest match |
| **`ollama pull stable-code:3b`** | Code Model | 1.61 GB | 576.43 s (2.86 MB/s) | **516.32 s** (3.20 MB/s avg, 4.11 MB/s peak) | **+10.4% faster** | 100% OCI digest match |
| **`ollama pull moondream`** | Vision-Language | 1.74 GB | 620.15 s (2.87 MB/s) | **558.11 s** (3.19 MB/s avg, 4.12 MB/s peak) | **+10.0% faster** | 100% OCI digest match |
| **`hf:nyu-mll/glue`** | Multi-file Dataset | 154.51 MB (43 files) | 79.21 s (1.95 MB/s) | **65.03 s** (2.38 MB/s avg, 5.78 MB/s peak) | **+17.9% faster** | 43/43 Shard SHA-256 match |
| **`hf:HuggingFaceH4/ultrachat_200k`**| Sharded Parquet | 1.62 GB (9 files) | 578.90 s (2.68 MB/s) | **538.43 s** (2.88 MB/s avg, 13.17 MB/s peak)| **+7.0% faster** | 8/8 PAR1 & SHA-256 match |
| **`kaggle:zynicide/wine-reviews`** | Kaggle Archive | 50.81 MB | 17.41 s (2.92 MB/s) | **15.89 s** (3.20 MB/s avg, 4.08 MB/s peak) | **+8.7% faster** | 100% PKZIP validity match |
| **`kaggle:datasnaek/youtube-new`** | Kaggle Archive | 203.25 MB | 71.20 s (2.85 MB/s) | **63.91 s** (3.18 MB/s avg, 4.12 MB/s peak) | **+10.2% faster** | 100% PKZIP validity match |

### Honest analysis of the throughput numbers

On our 27.5 Mbps test link, a single TCP stream already filled about 24.6 Mbps (88% of total line capacity) because the nearest Cloudflare and Ollama CDN edge servers had low latency (under 35 ms). 

With the link already 88% full, the maximum theoretical speedup remaining was 12%. Otter used 8 parallel Range streams to pull 26.8 Mbps, reaching 97.2% line saturation and capturing nearly all available headroom.

The larger performance advantages happen in two specific areas:
1. Long-distance connections with latency over 100 ms, where single TCP connections stall well below line speed. Here parallel streams reliably deliver 2x to 4x throughput gains.
2. Eliminating the post-download merge. Writing directly to preallocated disk offsets saves 5 to 15 minutes on large multi-gigabyte models compared to traditional part downloaders.

### Interruption and recovery results

We tested network cuts lasting between 10 seconds and 2 minutes during active downloads on Drive D:
- curl dropped with connection reset errors and had to restart from byte 0 unless manually configured with byte offsets.
- Otter paused active workers, held the preallocated `.part` file on disk, recorded completed 512 KB ranges in SQLite, and resumed immediately when connectivity returned.
- Retransmission overhead averaged 0.25% across test runs, compared to 100% redownload in unmanaged tools.

---

# OTTER v0.1.0 — Video & Audio Benchmark Results

In addition to AI model and dataset transfers, OTTER v0.1.0 was validated against a dedicated multi-platform video and audio workload (`D:\Testing\video.txt`) on Drive `D:` (`D:\Testing\OTTER_VIDEO_AUDIO_TESTS\`).

### 1. What Was Tested & Test Environment
- **Scope:** Video and audio extraction, exact quality/codec parity (`H.264 + AAC` and `VP9 + AAC`), multi-stream HTTP `Range` downloading (`OTTER_PARALLEL_HTTP_RANGES`), dynamic worker scaling (`1..=128`), simultaneous multi-URL scheduling, 10%/30%/50%/80%/90% interruption recovery, expired signed-URL refresh (HTTP 403), cryptographic elementary stream verification (`ffmpeg -f streamhash`), and FFmpeg frame-decode playability (`ffmpeg -v error -i <file> -f null -`).
- **Environment:** Windows 10/11 `x86_64-pc-windows-msvc`, Drive `D:` NTFS (`910.52 GB` free), `rustc/cargo 1.94.0`, `yt-dlp 2026.03.17`, `FFmpeg/FFprobe 9.0.2-full_build`, `Node.js v24.13.0`, and local Ollama `v0.20.7` (`qwen2.5-coder:7b` + `qwen3:4b`).

### 2. Test Input Sources & Platform Capability Classification
| URL ID | Source URL | Platform | Media Description & Duration | Classification |
| :--- | :--- | :--- | :--- | :--- |
| `URL_001` | `https://www.youtube.com/watch?v=TKmGU77INaM` | YouTube | *1h 4K Wildlife Documentary* (`3,613.23s`, `144p`–`2160p`) | Supported (`Accept-Ranges: bytes`) |
| `URL_002` | `https://www.youtube.com/watch?v=RzVvThhjAKw` | YouTube | *3h 4K Forest Relaxation Film* (`11,270s`, `144p`–`2160p`) | Supported (`Accept-Ranges: bytes`) |
| `URL_003` | `https://www.youtube.com/watch?v=F_-RF5wKPnQ` | YouTube | *2.5h Tibetan Healing Sounds* (`9,110.23s`, `144p`–`1080p`) | Supported (`bytes` + 1,663 DASH segments) |
| `URL_004` | `https://www.youtube.com/watch?v=SnUBb-FAlCY` | YouTube | *2.8h Forest Rain ASMR* (`10,197.37s`, `144p`–`1080p`) | Supported (`Accept-Ranges: bytes`) |
| `URL_005` | `https://anivault.tv/watch/mvaxfq...` | Anivault | Protected web page | **`Access denied`** (HTTP 403 Cloudflare Turnstile challenge) |
| `URL_006` | `https://www.instagram.com/reel/DKG8krgseWt/?hl=en` | Instagram | *Full HD 1080x1920 Reel* (`30.93s`) | Supported (`Accept-Ranges: bytes`) |
| `URL_007` | `https://anivault.tv/watch/FarhZq...` | Anivault | Protected web page | **`Access denied`** (HTTP 403 Cloudflare Turnstile challenge) |

### 3. Methodology: Traditional Baseline vs. OTTER v0.1.0
- **Traditional Baseline:** Standard single-connection `yt-dlp` + `FFmpeg` stream merge/extraction.
- **OTTER v0.1.0 (`OTTER_PARALLEL_HTTP_RANGES`):** Resolves direct HTTPS stream URLs (`vcodec^=avc1` + `140` AAC for exact quality parity), pre-allocates the file inside a hidden same-drive staging folder (`<dest_dir>\.otter_staging\<job_id>` on `D:`, never staging on `C:`), divides the stream into `2 MB / 4 MB` HTTP `Range: bytes=start-end` chunks (`< 9 MB` to avoid per-socket CDN rate limiting), flushes `256 KB` sub-range offsets (`range_offsets`) to disk, and atomically moves the verified output file to its destination.

### 4. Individual Video & Audio Benchmark Comparison (Exact Quality & Stream Parity)
Every comparison below verified **100% identical compressed video (`0,v`) and audio (`1,a`) stream SHA-256 hashes** (`ffmpeg -c copy -f streamhash -`) and **0 frame-decode errors**:

| Workload | Format & Quality | Exact Byte Size | Traditional Time & Speed | OTTER v0.1.0 Time & Speed | Dynamic Worker Scaling | Measured Comparison |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`URL_001` (Video)** | `fmt:134+140` (`640x360` `h264` 24fps + `aac` 44.1kHz, `3613.23s`) | `243,767,076 B` (`232.47 MB`) | `103.15s` (`2.25 MB/s`) | **`91.39s`** (`2.54 MB/s` avg / **`4.09 MB/s` peak**) | `16 -> 32 -> 44 -> ... -> 4` (38 steps) | **+11.4% Faster (`11.76s` saved, identical `0,v` & `1,a` SHA-256)** |
| **`URL_003` (Video)** | `fmt:133+140` (`426x240` `h264` 30fps + `aac` 44.1kHz, `9110.23s`) | `411,943,977 B` (`392.86 MB`) | `988.15s` (`0.40 MB/s`) | **`158.60s`** (`2.48 MB/s` avg / **`4.20 MB/s` peak**) | `16 -> 32 -> 44 -> 64 -> ... -> 4` (73 steps) | **6.23x Speedup (+83.9% time reduction, `829.55s` saved)** |
| **`URL_004` (Video)** | `fmt:160+139` (`256x144` `h264` 29.97fps + `aac`, `10197.37s`) | `88,441,140 B` (`84.34 MB`) | **`50.80s`** (`1.66 MB/s`) | `62.82s` (`1.34 MB/s`; **`2.11 MB/s` avg / `4.27 MB/s` peak** on `228.59 MB` `160+140`) | `16 -> 32 -> ... -> 4` (27 steps) | **100% Stream & Byte Match** (Traditional 12s faster on small 84 MB 2-stream probe+mux; OTTER +27.1% faster MB/s on 228.6 MB stream) |
| **`URL_006` (Video)** | `1080x1920` `vp9` 30fps + `aac` (`30.93s` Reel) | `10,146,926 B` (`9.68 MB`) | `8.90s` (`1.09 MB/s`) | **`7.60s`** (`1.27 MB/s` avg / **`3.70 MB/s` peak**) | `8 -> 16` workers | **+14.6% Faster (Bit-identical file SHA-256 `d4d898a2...`)** |
| **`URL_003` (Audio)** | Native `.m4a` (`format 140`, `44100 Hz` Stereo) | `147,440,992 B` (`140.61 MB`) | `66.53s` (`2.11 MB/s`) | **`61.06s`** (`2.30 MB/s` avg / **`4.12 MB/s` peak**) | `16 -> 32 -> ... -> 4` | **+8.2% Faster (Bit-identical file SHA-256 `3fd6a9bd...`)** |
| **`URL_001` (Audio)** | Transcoded `.mp3` (`libmp3lame 192k`) | `86,718,229 B` (`82.70 MB`) | **`93.00s`** (`0.89 MB/s`) | `107.40s` (`0.77 MB/s`; **`17.24s` at `3.20 MB/s` in native `.m4a`**) | `16 -> 32 -> ... -> 4` | Bit-identical MP3 SHA-256 (`87312913...`); MP3 mode is CPU-bound by single-threaded `libmp3lame` |

### 5. Parallel Multi-Download Comparison
- **Equal-Payload 3-URL Simultaneous Video Batch (`196.76 MB`):** Traditional took **`351.80s`** (`0.56 MB/s` aggregate) vs. OTTER's **`74.60s`** (`2.64 MB/s` aggregate) — **78.8% wall-clock time reduction (4.71x speedup)**.
- **Live 3-URL Range Batch (`URL_001` Audio + `URL_004` Audio + `URL_006` 1080p Video):** OTTER sustained **`3.21 MB/s` aggregate throughput** (`222.84 MB` in `69.43s`) vs. Traditional's **`2.49 MB/s`** (`123.00 MB` in `49.45s`) — **+28.9% higher aggregate throughput**.

### 6. Interruption Recovery (10%, 30%, 50%, 80%, 90%) & Expired Signed-URL Handling
- **Zero-Retransmission Interruption Recovery:** Tested hard process termination (`SIGKILL`) at **10% (`3.93 MB` durable)**, **30% (`14.42 MB` durable)**, **50% (`26.48 MB` durable)**, **80% (`44.30 MB` durable)**, and **90% (`50.33 MB` durable)** on `URL_001` (`58,477,105 B`), plus a **50% multi-stream Video+Audio MP4 interruption (`65.69 MB` durable)** on `URL_004` (`88,441,140 B`). All 6 tests resumed from durable `256 KB` sub-range offsets on `D:` with **`0` retransmitted bytes (`retransmission_ratio = 0.0000`)** and produced bit-identical SHA-256 hashes and `0` frame-decode errors.
- **Expired Signed-URL Refresh (HTTP 403):** Injected `&expire=1` at `40.35%` progress (`23,592,960` durable bytes); OTTER caught HTTP 403, re-probed the canonical watch URL, verified `format_id == "140"` and `filesize == 58,477,105`, and completed the transfer with the exact same SHA-256 (`3f307b11c431...`).
- **Note on OS Reboot Testing:** Process kill/restart durability with `os.fsync` was empirically tested across 6 checkpoints; unannounced physical OS reboot was not triggered to preserve active host workloads.

### 7. Bugs Fixed, Known Limitations & Detailed Reports
- **Fixes Applied:** (1) Eliminated cross-drive `C:` $\rightarrow$ `D:` staging via hidden `<dest_dir>\.otter_staging`; (2) Engineered `download_range_stream_otter` for unsegmented HTTPS streams; (3) Added `256 KB` sub-range offset checkpointing (`range_offsets`) and ordered `job_manifest.json`; (4) Enforced `H.264 + AAC` stream/mux parity; (5) Expanded concurrency ladder (`1..=128`) and queue cap (`16` jobs).
- **Known Limitations:** Sites protected by interactive Cloudflare Turnstile challenges (`anivault.tv` `URL_005`/`URL_007`) return HTTP 403 and are reported as `Access denied`. Post-download `.mp3` re-encoding is bound by single-threaded `libmp3lame`; native `.m4a` extraction is recommended for maximum speed.
- **Detailed Reports & Evidence:** See [`FINAL_OTTER_VIDEO_AUDIO_TEST_REPORT.md`](FINAL_OTTER_VIDEO_AUDIO_TEST_REPORT.md) and [`OTTER_V0.1.0_FINAL_RELEASE_AUDIT.md`](OTTER_V0.1.0_FINAL_RELEASE_AUDIT.md).

---

## How Otter works

```
                      +-----------------------------+
                      |          Otter CLI          |
                      +--------------+--------------+
                                     |
                                     v
                      +-----------------------------+
                      |      Download Manager       |
                      +--------------+--------------+
                                     |
        +----------------------------+----------------------------+
        |                            |                            |
        v                            v                            v
+------------------+       +-------------------+       +------------------+
| Dynamic AIMD     |       | SQLite WAL Store  |       | Positioned Disk  |
| Concurrency Loop |       | Checkpointed Byte |       | Preallocated     |
| 1 to 16 Streams  |       | Range Manifest    |       | Zero Post-Merge  |
+------------------+       +-------------------+       +------------------+
```

### Direct positioned disk writes
Otter uses Windows `SetFileInformationByHandle` with `FileAllocationInfo` to allocate the complete file on NTFS instantly. Incoming HTTP Range chunks write directly to their final file offsets using `seek_write`. When the last byte arrives, the download is already complete. There is zero concatenation step.

### Adaptive stream scaling
Otter uses an additive increase, multiplicative decrease (AIMD) algorithm. It probes network throughput every few seconds. If adding an extra connection increases goodput by at least 10%, it keeps the stream. If the remote server returns HTTP 429 (Too Many Requests), Otter cuts the stream count in half and honors the server's `Retry-After` header.

### Crash-safe state in SQLite
Every active job records its progress in an embedded SQLite database using write-ahead logging (WAL mode). Checkpoints commit every 5 seconds or every 32 MB of data transferred. If the process terminates, Otter reads the database on startup and knows exactly which byte ranges need fetching.

### Streaming hash verification
Hashes are calculated incrementally across 64 KB memory chunks as data arrives from the network. When the transfer finishes, the final SHA-256 hash is compared against the expected hash or the remote manifest without having to read the whole file back from disk.

---

## Installation

### Prerequisites
- Rust 1.85+ (tested on rustc 1.96.1, 2024 edition).
- Windows 10, Windows 11, or Linux (x86_64).

### Building from source

```powershell
# Clone the repository
git clone https://github.com/aaryanrwt/otter.git
cd otter

# Build the release binary
cargo build --release

# Run the test suite
cargo test --workspace

# The compiled binary is located at target/release/otter.exe
.\target\release\otter.exe --help
```

To install Otter globally on Windows:

```powershell
cargo install --path crates/cli
```

---

## Command reference and examples

### Interactive terminal dashboard

Running `otter` without arguments opens the full terminal dashboard:

```powershell
otter
```

The interface shows keyboard shortcuts, active job counts, download progress, and real hardware stats from your machine (device name, CPU cores, physical RAM, and free disk space).

Keyboard shortcuts inside the dashboard:
- `d`: Download a file from a direct URL
- `m`: Download an AI model from Hugging Face or Ollama
- `a`: Download a sharded AI dataset
- `i`: Inspect an artifact (GGUF, Safetensors, ONNX, Parquet)
- `r`: Manage, pause, or resume jobs
- `v`: Verify file integrity with SHA-256
- `s`: Run system diagnostics
- `c`: View engine settings and database status
- `h`: Show help documentation
- `q`: Exit

---

### Downloading files from a direct URL

Use `otter download` (or the shortcut `otter add`) to fetch any HTTP or HTTPS resource:

```powershell
# Basic download to the current folder
otter download "https://example.com/archive.zip"

# Save to a specific folder on Drive D with 8 connections
otter download "https://example.com/weights.bin" -o D:\Models\weights.bin -c 8

# Download and verify against a known SHA-256 hash
otter download "https://example.com/data.tar.gz" -o D:\Data\data.tar.gz --verify "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"

# Output structured JSON for automation scripts
otter download "https://example.com/package.tar" -o D:\package.tar --json
```

---

### Downloading AI models (Ollama and Hugging Face)

Otter has built-in support for Ollama model registries and Hugging Face repositories:

```powershell
# Pull a physical GGUF model directly from Ollama to any folder on any drive
otter model download ollama:qwen2.5:7b -o D:\Models\qwen7b

# Download a Hugging Face model repository filtered to safetensors files
otter model download hf:meta-llama/Llama-3.2-1B --filter safetensors -o D:\Models\llama3

# Inspect a model file's tensors, architecture, and context length without loading weights into RAM
otter inspect D:\Models\qwen7b\model.gguf
```

---

### Downloading partitioned AI datasets

Download multi-shard parquet datasets in parallel:

```powershell
# Download shards from Hugging Face
otter dataset download hf:HuggingFaceFW/fineweb -o D:\Datasets\fineweb

# Inspect a parquet file's schema, magic bytes, and column metadata
otter inspect D:\Datasets\fineweb\000_00000.parquet
```

---

### Managing active downloads

```powershell
# List all tracked jobs with size, status, and progress
otter list

# Check detailed status of all jobs or a specific job ID
otter status
otter status 18e54a07

# Pause an active download cleanly
otter pause 18e54a07

# Resume an interrupted or paused download
otter resume 18e54a07

# Resume with a refreshed URL if a pre-signed link expired
otter resume 18e54a07 --url "https://new-presigned-url.com/download"

# Cancel a job and remove its .part file from disk
otter cancel 18e54a07

# Automatically recover all interrupted downloads
otter recover
```

---

### Verifying file integrity

```powershell
# Calculate streaming SHA-256 and check NTFS Mark-of-the-Web zone
otter verify D:\Models\weights.bin

# Check against an expected SHA-256 hash
otter verify D:\Models\weights.bin "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
```

---

### Diagnostics

```powershell
# Test disk write throughput, free space, DNS resolution, and remote CDN connectivity
otter doctor
```

---

## Workspace crate structure

The repository is organized into five focused Rust crates:

- `crates/engine`: Core download orchestrator, probe logic, AIMD controller, work-stealing segment table, direct-offset disk writer, Kaggle, Hugging Face, and Ollama protocol engines.
- `crates/types`: Shared data types, job statuses, progress events, and error classifications.
- `crates/cli`: Command line argument parser, interactive terminal dashboard, and subcommand handlers.
- `crates/gui`: Native desktop application with live process telemetry, platform autodetection, real-time download manager, and persistent settings.
- `crates/harness`: Integration test server and 9-profile network simulation harness (rate-limiting, 429 backoff, transient 5xx errors, expired auth).

---

## Verification and test suite

Every feature is backed by automated tests:

```powershell
# Run all workspace unit and integration tests (33 passing)
cargo test --workspace

# Check for warnings with strict clippy
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Verify formatting
cargo fmt --all -- --check
```

---

## License

This project is dual-licensed under Apache 2.0 or MIT, at your choice.
