# Otter CLI implementation status

This document maps all implemented subsystems in Otter v0.1.0 against their source files and verification methods.

## Architectural components

### Download orchestrator and network engine (`crates/engine`)

- Probe subsystem (`probe.rs`): sends HTTP HEAD and Range probes to detect server capabilities, byte-range support, total file length, and ETag identifiers.
- Adaptive concurrency controller (`adaptive.rs`): dynamic AIMD controller that probes goodput every 2.5 seconds. Increases active streams when bandwidth improves by at least 10%, stabilizes at plateaus, and halves connections on HTTP 429 rate limits.
- Worker pool (`orchestrator.rs`): Tokio-based task pool with dynamic work-stealing, a per-host connection semaphore (capped at 16 connections), and tail starvation prevention for the final 5% of a file.
- Disk I/O subsystem (`disk.rs`): preallocates files on NTFS via Windows `SetFileInformationByHandle`, writes chunks directly to assigned offsets using `seek_write`, writes Mark-of-the-Web zone streams, and checks free disk space before transfers begin.
- Persistence store (`manifest.rs`): embedded SQLite database in WAL mode with `PRAGMA synchronous = NORMAL`. Automatically tunes autocheckpoint limits based on disk media (8,000 pages for HDD, 1,000 pages for SSD).
- Diagnostics (`doctor.rs`): measures direct write throughput, available disk space, DNS lookup latency, and remote CDN connectivity.
- AI artifact parsers (`artifact.rs`): streaming parsers for GGUF metadata, Safetensors headers, ONNX protobuf wire definitions, and Parquet schema footers.

### Command line interface (`crates/cli`)

- Command parser (`clap`): 14 implemented subcommands, including aliases such as `otter add` for `otter download` and `otter export` for `otter export-manifest`.
- Interactive terminal dashboard: two-column responsive interface built on `crossterm`:
  - Left panel: 10 capabilities with shortcut keys `[d]`, `[m]`, `[a]`, `[i]`, `[r]`, `[v]`, `[s]`, `[c]`, `[h]`, `[q]`.
  - Right panel: three cards for Quick Actions, live Job Overview (active, paused, completed, total counts), and System hardware specs queried from the host OS.
  - Keyboard navigation: supports arrow keys, Enter to select, Esc to return, and direct hotkeys.
  - Screen rendering: uses relative cursor rewinding to eliminate flicker.
- Subcommand handlers: execution paths for download, resume, pause, cancel, verify, inspect, doctor, status, list, export-manifest, model download, dataset download, manage, and config.
- Exit codes: standard exit codes matching POSIX conventions.
