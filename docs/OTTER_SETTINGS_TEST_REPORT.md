# OTTER v0.1.0 — Settings Test Report

## Environment
- OS: Windows 11 Enterprise (x86_64-pc-windows-msvc)
- Shell: PowerShell 7 / Windows Terminal
- Rust Edition: 2024 / 1.85+
- Binary Target: `target/debug/otter-gui.exe`

## Test Execution Summary

### 1. Engine & Core Verification
```
running 5 tests
test adaptive::tests::test_aimd_multiplicative_decrease_on_429 ... ok
test artifact::onnx_tests::test_native_onnx_inspection ... ok
test disk::tests::test_file_writer_preallocate_and_motw ... ok
test adaptive::tests::test_aimd_additive_increase ... ok
test adaptive::tests::test_aimd_plateau_detection ... ok

test result: ok. 5 passed; 0 failed; finished in 0.11s
```

### 2. Artifact & Manifest Verification
```
running 5 tests
test test_ollama_tag_parsing ... ok
test test_parquet_inspection_validation ... ok
test test_safetensors_inspection_validation ... ok
test test_reproducibility_manifest_lifecycle ... ok
test test_doctor_engine_diagnostics ... ok

test result: ok. 5 passed; 0 failed; finished in 1.02s
```

### 3. Disk Preallocation & Recovery Verification
```
running 4 tests
test test_tail_starvation_work_stealing_active ... ok
test test_error_classification_and_backoff ... ok
test test_disk_space_query_and_safeguard ... ok
test test_sha256_buffer_streaming ... ok

test result: ok. 4 passed; 0 failed; finished in 0.13s
```

### 4. Network Profile & Bandwidth Tests
```
running 8 tests
test test_profile_chunked_unknown ... ok
test test_profile_no_range ... ok
test test_profile_expired_auth_and_url_refresh ... ok
test test_profile_changed_resource_never_splice ... ok
test test_profile_transient_5xx ... ok
test test_profile_fast_stable ... ok
test test_profile_rate_limited_429 ... ok
test test_profile_throttled_per_conn ... ok

test result: ok. 8 passed; 0 failed; finished in 2.48s
```

## Settings Feature Test Scenarios

| Scenario | Tested Action | Expected Result | Pass / Fail |
| :--- | :--- | :--- | :--- |
| ST-01 | Toggle "Start with Windows" | Registry entry added to `HKCU\...\Run` | PASS |
| ST-02 | Untoggle "Start with Windows" | Registry entry deleted without console flash | PASS |
| ST-03 | Click "Change" folder button | Native Win32 FolderBrowserDialog opens | PASS |
| ST-04 | Select directory in folder dialog | Path updates in UI and `gui_settings.json` | PASS |
| ST-05 | Change concurrent downloads to 8 | Persists to disk atomically | PASS |
| ST-06 | Change language to Spanish | Dropdown retains selection across restarts | PASS |
| ST-07 | Reset settings to defaults | Reverts to 5 downloads, No limit, warm ivory | PASS |
| ST-08 | Verify sleeping mascot display | Inlined base64 PNG renders with floating "z z z" | PASS |
| ST-09 | Verify Engine navigation | Engine tab absent from sidebar and viewport | PASS |
