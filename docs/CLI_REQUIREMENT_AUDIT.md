# Otter CLI requirement audit

This document tracks the requirements for Otter v0.1.0 against their source files and automated test verification.

## Requirement traceability matrix

| ID | Requirement | Rust source file | Verification method | Status |
| :--- | :--- | :--- | :--- | :---: |
| REQ-01 | CLI and TUI scope only (no GUI) | `crates/cli`, `crates/engine`, `crates/types` | Workspace audit confirms zero GUI or Tauri dependencies | Verified |
| REQ-02 | HTTP Range and capability probe | `crates/engine/src/probe.rs` | Unit tests `test_profile_no_range` and `test_profile_fast_stable` | Verified |
| REQ-03 | Single-stream sequential fallback | `crates/engine/src/orchestrator.rs` | Integration test against non-Range servers | Verified |
| REQ-04 | Work-stealing segment division | `crates/engine/src/segment.rs` | Property test `test_work_stealing_split` | Verified |
| REQ-05 | Direct positioned disk writes | `crates/engine/src/disk.rs` | Disk write test confirming zero post-download file concatenation | Verified |
| REQ-06 | Tail starvation prevention | `crates/engine/src/segment.rs` | Unit test `test_tail_starvation_work_stealing_active` | Verified |
| REQ-07 | SQLite WAL manifest store | `crates/engine/src/manifest.rs` | Persistence test `test_manifest_job_crud_and_persistence` | Verified |
| REQ-08 | Crash checkpoint protocol | `crates/engine/src/orchestrator.rs` | Flush and sync validation on 5-second intervals | Verified |
| REQ-09 | Resumption re-probe validation | `crates/engine/src/orchestrator.rs` | Integration test `test_profile_changed_resource_never_splice` | Verified |
| REQ-10 | Error classification and backoff | `crates/engine/src/error.rs` | Unit test `test_error_classification_and_backoff` | Verified |
| REQ-11 | Adaptive AIMD concurrency | `crates/engine/src/adaptive.rs` | Tests `test_aimd_additive_increase` and `test_aimd_multiplicative_decrease_on_429` | Verified |
| REQ-12 | Rate limit handling (HTTP 429) | `crates/engine/src/orchestrator.rs` | Test `test_profile_rate_limited_429` | Verified |
| REQ-13 | Host connection semaphore cap | `crates/engine/src/orchestrator.rs` | Semaphore boundary test capping active host sockets at 16 | Verified |
| REQ-14 | Diagnostic suite (`doctor`) | `crates/engine/src/doctor.rs` | CLI `otter doctor` and test `test_doctor_engine_diagnostics` | Verified |
| REQ-15 | URL token refresh on resume | `crates/cli/src/main.rs` | Integration test `test_profile_expired_auth_and_url_refresh` | Verified |
| REQ-16 | Developer CLI and exit codes | `crates/cli/src/main.rs` | CLI test suite verifying standard POSIX exit codes | Verified |
| REQ-17 | Streaming SHA-256 verifier | `crates/engine/src/verify.rs` | Test `test_streaming_sha256_verification` | Verified |
| REQ-18 | Free disk space check | `crates/engine/src/disk.rs` | Windows `GetDiskFreeSpaceExW` test `test_disk_space_query_and_safeguard` | Verified |
| REQ-19 | Windows sleep prevention | `crates/engine/src/platform/win.rs` | Win32 `SetThreadExecutionState` API integration | Verified |
| REQ-20 | Mark-of-the-Web zone stream | `crates/engine/src/disk.rs` | NTFS ADS test `test_file_writer_preallocate_and_motw` | Verified |
| REQ-21 | Local in-process control plane | `crates/cli/src/main.rs` | Verified zero open listening ports or background sockets | Verified |
| AI-01 | GGUF metadata parser | `crates/engine/src/artifact.rs` | Unit test verifying header magic and tensor key-value parsing | Verified |
| AI-02 | Safetensors header inspector | `crates/engine/src/artifact.rs` | Test `test_safetensors_inspection_validation` | Verified |
| AI-03 | Parquet schema inspector | `crates/engine/src/artifact.rs` | Test `test_parquet_inspection_validation` | Verified |
| AI-04 | Pure-Rust ONNX wire decoder | `crates/engine/src/artifact.rs` | Test `test_native_onnx_inspection` | Verified |
| DEC-04 | Rotational media autotuning | `crates/engine/src/disk.rs` | Autocheckpoint tuning (8,000 pages on HDD, 1,000 on SSD) | Verified |
| HAR-01 | 9-profile network harness | `crates/harness/src/lib.rs` | 8 integration tests passing against simulated network profiles | Verified |
