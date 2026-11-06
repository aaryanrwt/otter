# Otter CLI architecture

## System overview

Otter is a download engine and interactive terminal interface written in Rust (2024 edition). It is built for downloading large files, including multi-gigabyte GGUF models, Safetensors neural network weights, sharded Parquet datasets, and general binary archives.

```
+------------------------------------------------------------------------------------+
|                                    OTTER CLI / TUI                                 |
|          crossterm (raw mode, no flicker)  | clap v4 (subcommands and aliases)     |
+------------------------------------------------------------------------------------+
                                           |
                                           v
+------------------------------------------------------------------------------------+
|                                  DOWNLOAD MANAGER                                  |
|               concurrency semaphores | broadcast progress channels                 |
+------------------------------------------------------------------------------------+
       |                                   |                                  |
       v                                   v                                  v
+------------------+             +-------------------+             +------------------+
|  ADAPTIVE AIMD   |             |    SQLITE WAL     |             |   DISK IO / MOTW |
| Concurrency Loop |             |  Manifest Store   |             | Positioned Writes|
| Backoff & Retry  |             | Persistent Ranges |             | Zone.Identifier  |
+------------------+             +-------------------+             +------------------+
       |                                   |                                  |
       v                                   v                                  v
+------------------+             +-------------------+             +------------------+
|  HTTP 1.1 / 2    |             | ARTIFACT PARSERS  |             |  STREAMING HASH  |
| Range Transfers  |             | GGUF, Safetensors |             | SHA-256 Buffers  |
| Server-Polite B2 |             | ONNX and Parquet  |             | Incremental Calc |
+------------------+             +-------------------+             +------------------+
```

---

## Subsystem details

### Terminal user interface

The TUI uses `crossterm` in raw mode with ANSI escape sequence handling:

- Two-column responsive layout:
  - Left panel: 10 primary capabilities matching the Part 1 CLI specification, with a purple highlight and keyboard shortcuts `[d]`, `[m]`, `[a]`, `[i]`, `[r]`, `[v]`, `[s]`, `[c]`, `[h]`, `[q]`.
  - Right column: three cards covering Quick Actions, Job Overview (active, paused, completed, total counts), and System (device name, CPU cores, physical RAM, and free disk space queried live from the host OS).
  - Responsive layout: automatically switches to a stacked single-column view when the terminal width is under 105 columns.
  - Redrawing: uses relative cursor repositioning (`cursor::MoveUp`) to eliminate screen flicker.

### Concurrency and network engine

- Adaptive AIMD controller: probes network throughput at regular intervals. It uses additive increase (+1 connection when goodput improves by at least 10%) and multiplicative decrease (halving active connections on HTTP 429 responses or backpressure) to avoid server-side rate limits.
- Server politeness: reads `Retry-After` response headers and applies jittered exponential backoff intervals during rate limiting.
- Host rate limiter: uses a tokio semaphore to cap connections per domain (default: 16 connections per host) to prevent socket starvation.

### Storage and crash safety

- SQLite manifest in WAL mode: tracks jobs, file metadata, and completed byte ranges in SQLite with `PRAGMA synchronous = NORMAL`. The autocheckpoint interval tunes automatically for storage media (8,000 pages for rotational HDDs, 1,000 pages for SSDs).
- Direct-offset positioned writes: writes incoming HTTP range chunks directly to target offsets via Windows `seek_write` (`WriteFile` at explicit file offsets). This eliminates the post-download file merge phase entirely.
- Windows Mark-of-the-Web: writes the `:Zone.Identifier` alternate data stream (`ZoneId=3`) with `HostUrl` directly onto NTFS storage when a download finishes.

### AI artifact parsers

- GGUF parser: binary parser that reads the `GGUF` magic bytes, version, tensor counts, and metadata key-value pairs without loading weights into memory.
- Safetensors parser: reads the 8-byte length prefix and JSON metadata to inspect tensor names, shapes, and datatypes.
- ONNX parser: streaming Protobuf decoder that extracts graph nodes, inputs, outputs, and opset imports.
- Parquet parser: verifies the `PAR1` magic header and footer bytes along with thrift metadata length.
