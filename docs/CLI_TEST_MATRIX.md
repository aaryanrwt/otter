# Otter CLI test matrix

This document lists the test suites, verification protocols, and live download checks used to validate Otter v0.1.0.

## Automated test suites

### Unit tests in `crates/engine`

- `test_aimd_additive_increase`: Verifies that achieving a 10% or greater throughput gain spawns an additional connection.
- `test_aimd_multiplicative_decrease_on_429`: Verifies that an HTTP 429 response cuts active streams in half and triggers backoff.
- `test_aimd_plateau_detection`: Confirms that concurrency stabilizes when additional streams stop improving overall throughput.
- `test_native_onnx_inspection`: Checks streaming Protobuf decoding of ONNX graph inputs, outputs, nodes, and opsets.
- `test_file_writer_preallocate_and_motw`: Validates NTFS preallocation via `set_len` and writing the Zone.Identifier alternate data stream.

### Artifact and AI tests (`crates/engine/tests/artifact_and_ai_tests.rs`)

- `test_ollama_tag_parsing`: Verifies parsing of Ollama model identifiers in `ollama:<model>[:tag]` format.
- `test_safetensors_inspection_validation`: Checks reading of 8-byte length headers and JSON tensor descriptor parsing.
- `test_parquet_inspection_validation`: Checks parsing of the Parquet schema and column definitions from file footers.
- `test_reproducibility_manifest_lifecycle`: Tests generation and verification of SHA-256 reproducibility manifests.
- `test_doctor_engine_diagnostics`: Validates disk write benchmarks, directory permission checks, and network connectivity tests.

### Disk and recovery tests (`crates/engine/tests/disk_and_recovery_tests.rs`)

- `test_error_classification_and_backoff`: Checks retry policies across transient, fatal, rate-limited, and expired authentication errors.
- `test_tail_starvation_work_stealing_active`: Verifies that the final 5% of a file prevents spawning excessive connections while allowing active workers to steal split slices.
- `test_disk_space_query_and_safeguard`: Checks free storage detection through the Windows `GetDiskFreeSpaceExW` API.
- `test_sha256_buffer_streaming`: Validates incremental 64 KB memory hashing against standard NIST SHA-256 test vectors.

### Network harness profile tests (`crates/harness/tests/profile_tests.rs`)

- `test_profile_fast_stable`: Baseline transfer test across 8 concurrent Range connections.
- `test_profile_no_range`: Validates single-connection streaming when a server returns HTTP 200 without range support.
- `test_profile_rate_limited_429`: Tests AIMD worker downscaling when a remote server returns HTTP 429.
- `test_profile_transient_5xx`: Checks automated exponential retries during transient 500 and 503 gateway errors.
- `test_profile_expired_auth_and_url_refresh`: Verifies transition to waiting status and resumption when a refreshed URL is provided.
- `test_profile_changed_resource_never_splice`: Verifies that changing ETags invalidate stale partial files instead of corrupting data.
- `test_profile_chunked_unknown`: Single-connection streaming for chunked responses with unknown content length.
- `test_profile_throttled_per_conn`: Dynamic worker splitting under per-socket rate limiting.

---

## Real-world download verification

| Test ID | Resource | Target size | Verification method |
| :--- | :--- | :--- | :--- |
| TEST-RW-01 | Public binary benchmark file | 50 MB | Multi-stream HTTP Range download with SHA-256 validation |
| TEST-RW-02 | Public binary benchmark file | 100 MB | Concurrency adaptation and direct preallocation |
| TEST-RW-03 | Public binary benchmark file | 200 MB | Memory footprint verification staying under 64 MB RAM |
| TEST-RW-04 | Concurrent downloads | 3 files (138 MB total) | 3 simultaneous download processes writing to separate files without conflicts |
| TEST-RW-05 | Machine learning model weights | 440 MB to 1.04 GB | Hugging Face safetensors and Ollama GGUF downloads with metadata inspection |
| TEST-RW-06 | Sharded dataset shard | 45 MB to 548 MB | Hugging Face Parquet shard downloads with column schema verification |
| TEST-RW-07 | Interruption and resume | 45 MB | Process killed mid-transfer, then resumed from last byte offset |
| TEST-RW-08 | Interactive pause and resume | 50 MB | Paused during transfer, flushed to disk, and cleanly resumed |
| TEST-RW-09 | Cancellation and cleanup | 50 MB | Cancelled during transfer, confirming automatic deletion of the `.part` file |
| TEST-RW-10 | Full CLI command check | All 14 subcommands | Executed each subcommand to confirm clean exit code 0 |
