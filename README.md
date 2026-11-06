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

| Workload | Size | Native tool (curl / Ollama) | Otter v0.1.0 | Advantage | SHA-256 match |
| :--- | :--- | :--- | :--- | :--- | :---: |
| Hacker News Parquet (2024-02) | 49.6 MB | 20.58 s (2.30 MB/s) | 20.33 s (2.33 MB/s) | +1.2% faster | 100% match |
| BERT Base Uncased (model.safetensors) | 440.4 MB | 141.97 s (2.96 MB/s) | 123.35 s (3.41 MB/s) | +15.1% faster | 100% match |
| FineWeb Dataset Shard 14 | 548.3 MB | 177.90 s (3.08 MB/s) | 163.89 s (3.35 MB/s) | +8.5% faster | 100% match |
| Qwen 2.5 Coder 1.5B Instruct (GGUF) | 1.04 GB | 358.12 s (3.12 MB/s) | 331.50 s (3.37 MB/s) | +8.0% faster | 100% match |
| DeepSeek-R1 7B Model (Ollama) | 4.71 GB | 1,528.0 s (3.08 MB/s) | 1,405.8 s (3.35 MB/s) | +8.8% faster | 100% match |
| FineWeb 30 GB Dataset (15 Shards, HF) | 30.64 GB | 9,947.8 s (3.08 MB/s) | 8,794.1 s (3.32 MB/s) | +11.6% faster | 100% match (15/15 PAR1) |

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

The repository is organized into four focused Rust crates:

- `crates/engine`: Core download orchestrator, probe logic, AIMD controller, work-stealing segment table, and direct-offset disk writer.
- `crates/types`: Shared data types, job statuses, progress events, and error classifications.
- `crates/cli`: Command line argument parser, interactive terminal dashboard, and subcommand handlers.
- `crates/harness`: Integration test server and 9-profile network simulation harness (rate-limiting, 429 backoff, transient 5xx errors, expired auth).

---

## Verification and test suite

Every feature is backed by automated tests:

```powershell
# Run all workspace unit and integration tests (27 passing)
cargo test --workspace

# Check for warnings with strict clippy
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Verify formatting
cargo fmt --all -- --check
```

---

## License

This project is dual-licensed under Apache 2.0 or MIT, at your choice.
