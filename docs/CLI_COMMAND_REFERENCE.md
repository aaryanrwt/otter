# Otter CLI command reference

This document covers all commands, options, environment variables, and exit codes in Otter v0.1.0.

## Global options

- `-h, --help`: Print help information.
- `-V, --version`: Print version information.

---

## Primary commands

### `otter download <URL>` (alias: `otter add`)

Downloads a file from a direct HTTP or HTTPS URL using parallel HTTP Range requests and adaptive AIMD concurrency.

Arguments and options:
- `url`: Direct download URL (required).
- `-o, --output <PATH>`: Destination file or folder path.
- `-c, --connections <N>`: Maximum parallel TCP streams (default: 8).
- `-v, --verify <SHA256>`: Expected SHA-256 checksum for post-download integrity check.
- `--json`: Emit machine-readable JSON progress events.
- `-q, --quiet`: Suppress non-essential progress output.

Example:
```powershell
otter download "https://example.com/model.safetensors" -o D:\Models\model.safetensors -c 8
```

---

### `otter resume <JOB_ID>`

Resumes an interrupted or paused download job from its last verified byte offset.

Arguments and options:
- `id`: Job ID from `otter list`.
- `-c, --connections <N>`: Maximum parallel connections (default: 8).
- `-u, --url <URL>`: Refreshes an expired pre-signed URL or authentication token.

Example:
```powershell
otter resume 18e54a07
```

---

### `otter pause <JOB_ID>`

Pauses an active download job in flight, flushing all in-memory buffers to disk and committing byte ranges to the SQLite WAL.

Arguments:
- `id`: Job ID.

Example:
```powershell
otter pause 18e54a07
```

---

### `otter cancel <JOB_ID>`

Cancels an active or paused download, marks the job as cancelled in the database, and deletes the temporary `.part` file from disk.

Arguments:
- `id`: Job ID.

Example:
```powershell
otter cancel 18e54a07
```

---

### `otter list [--json]`

Lists all recorded download jobs in the SQLite manifest, including filename, size, status, and completion percentage.

Options:
- `--json`: Output full job list in formatted JSON.

Example:
```powershell
otter list
```

---

### `otter status [<JOB_ID>] [--json]`

Displays aggregate engine metrics (active, paused, completed, and total jobs), or detailed metrics for a specific job ID.

Arguments and options:
- `id`: Optional Job ID.
- `--json`: Output metrics as JSON.

Example:
```powershell
otter status
```

---

### `otter verify <FILE> [EXPECTED] [--json]`

Calculates the streaming SHA-256 hash across 64 KB memory chunks without loading the entire file into RAM, and checks the NTFS Mark-of-the-Web zone stream on Windows.

Arguments and options:
- `file`: Path to the file or reproducibility manifest.
- `expected`: Optional 64-character hexadecimal SHA-256 string.
- `--json`: Output verification result as JSON.

Example:
```powershell
otter verify D:\Models\weights.bin "fc733e3aa0f1fa7566166a7a70d623686a817aa470db6bc05e231a53781fdb63"
```

---

### `otter export-manifest <JOB_ID> [--json]` (alias: `otter export`)

Exports a reproducible JSON manifest recording source URL, ETag, completed byte ranges, file size, and checksums.

Arguments and options:
- `id`: Job ID.
- `--json`: Emit raw JSON only.

Example:
```powershell
otter export-manifest 18e54a07
```

---

### `otter doctor [--verbose] [--json]`

Runs diagnostics on local disk write speed, available storage capacity, NTFS alternate data stream permissions, DNS resolution latency, and remote CDN connectivity.

Example:
```powershell
otter doctor
```

---

### `otter inspect <PATH> [--json]`

Inspects metadata from local AI artifacts without loading full weights into RAM. Supports GGUF architecture and context length, Safetensors tensor shapes, ONNX graph nodes and opset versions, and Parquet column schemas.

Arguments and options:
- `path`: Path to the artifact file.
- `--json`: Output metadata tree as JSON.

Example:
```powershell
otter inspect D:\Models\qwen.gguf
```

---

### `otter model download <ID> [-o <PATH>] [--filter <PATTERN>]`

Downloads model packages from the Ollama registry (`ollama:<tag>`) or Hugging Face repositories (`hf:<repo_id>`).

Arguments and options:
- `model_id`: Model identifier (e.g. `ollama:qwen2.5:7b` or `hf:meta-llama/Llama-3.2-1B`).
- `-o, --output <PATH>`: Destination directory on any local drive.
- `--filter <PATTERN>`: File pattern filter (e.g. `safetensors` or `gguf`).
- `-c, --connections <N>`: Maximum connections per file (default: 8).

Example:
```powershell
otter model download ollama:qwen2.5:7b -o D:\Models\qwen7b
```

---

### `otter dataset download <ID> [-o <PATH>]`

Downloads partitioned, multi-shard datasets from Hugging Face (`hf:<dataset_id>`) in parallel.

Arguments and options:
- `dataset_id`: Dataset identifier (e.g. `hf:HuggingFaceFW/fineweb`).
- `-o, --output <PATH>`: Destination directory.
- `--subset <NAME>`: Dataset subset or split.

Example:
```powershell
otter dataset download hf:HuggingFaceFW/fineweb -o D:\Datasets\fineweb
```

---

### `otter manage`

Opens an interactive terminal table for browsing, pausing, resuming, and removing jobs using keyboard navigation.

---

### `otter config`

Displays active engine settings, the path to the SQLite WAL database, disk checkpoint thresholds, and connection limits.

---

## Exit codes

| Code | Name | Meaning |
| :--- | :--- | :--- |
| `0` | `Success` | Command finished successfully |
| `1` | `GeneralFailure` | Unhandled error or internal exception |
| `2` | `InvalidArguments` | Missing or unrecognized command line option |
| `3` | `NetworkFailure` | DNS resolution, TCP socket, or HTTP connection error |
| `4` | `IntegrityFailure` | Computed SHA-256 hash did not match expected checksum |
| `5` | `InsufficientStorage` | Target volume does not have enough free disk space |
| `6` | `AuthenticationFailure` | Missing or invalid API credentials |
| `7` | `Interrupted` | Process cancelled by user (Ctrl+C) |
| `8` | `UnsupportedFeature` | Server does not support required feature (e.g. byte ranges) |
