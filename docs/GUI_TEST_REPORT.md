# OTTER v0.1.0 — Desktop GUI Test Report
**Document Version:** 1.0.0  
**Status:** All 31 Tests Passing (100% Pass Rate)  
**Execution Environment:** Windows 11 Build 26300 (x86_64), Rust 1.83.0, WebView2 Runtime

---

## 1. Test Suite Summary

```
Package           Total Tests   Passed   Failed   Ignored   Duration
----------------------------------------------------------------------
engine (lib)                5        5        0         0      0.14s
artifact_and_ai_tests       5        5        0         0      0.94s
disk_and_recovery_tests     4        4        0         0      0.28s
manifest_tests              1        1        0         0      0.07s
segment_tests               3        3        0         0      0.02s
verify_tests                1        1        0         0      0.20s
gui (bin)                   4        4        0         0      2.39s
profile_tests               8        8        0         0      2.52s
----------------------------------------------------------------------
TOTAL                      31       31        0         0      6.56s
```

---

## 2. Detailed Test Case Results

### 2.1 Native Engine & Adaptive Concurrency
- `adaptive::tests::test_aimd_multiplicative_decrease_on_429`: **PASS** — Verifies connection backoff on HTTP 429 Too Many Requests.
- `adaptive::tests::test_aimd_additive_increase`: **PASS** — Verifies gradual additive connection increase when throughput is stable.
- `adaptive::tests::test_aimd_plateau_detection`: **PASS** — Detects network saturation and halts wasteful stream addition.
- `disk::tests::test_file_writer_preallocate_and_motw`: **PASS** — Verifies sparse preallocation and Windows Mark-of-the-Web security tagging.
- `artifact::onnx_tests::test_native_onnx_inspection`: **PASS** — Verifies ONNX model tensor inspection.

### 2.2 Artificial Intelligence & Artifacts
- `test_ollama_tag_parsing`: **PASS** — Verifies Ollama tag resolution and manifest discovery.
- `test_parquet_inspection_validation`: **PASS** — Verifies Apache Parquet dataset schema reading.
- `test_safetensors_inspection_validation`: **PASS** — Verifies Hugging Face safetensors tensor inventory.
- `test_reproducibility_manifest_lifecycle`: **PASS** — Verifies manifest export and audit trail.
- `test_doctor_engine_diagnostics`: **PASS** — Verifies internal engine diagnostic self-tests.

### 2.3 Disk Integrity & Crash Recovery
- `test_error_classification_and_backoff`: **PASS** — Verifies non-fatal network recovery vs fatal disk full errors.
- `test_tail_starvation_work_stealing_active`: **PASS** — Validates dynamic work-stealing when slow ranges stall.
- `test_disk_space_query_and_safeguard`: **PASS** — Verifies pre-allocation bounds check before initiating download.
- `test_sha256_buffer_streaming`: **PASS** — Verifies streaming SHA-256 chunk hashing.

### 2.4 GUI & Telemetry Integration
- `activity::tests::test_live_detection_toggle_behavior`: **PASS** — Verifies live detection toggle state updates in memory.
- `activity::tests::test_resolve_fallback_icon`: **PASS** — Verifies default folder fallback icon when brand logo is absent.
- `activity::tests::test_resolve_known_icons`: **PASS** — Verifies exact matching of Chrome, Steam, Epic, HF, GitHub logos.
- `activity::tests::test_activity_with_running_processes`: **PASS** — Verifies Win32 Toolhelp32 process snapshot scanning.

### 2.5 Real End-to-End Media Downloads
- **YouTube 4K Test URL (`QQxBi-ntc_w`):** Probed real stream info in 5.8s; extracted 2160p down to 144p; downloaded and remuxed MP4 container (8.99 MB) with clean line-delimited JSON progress. Result: **PASS**.
