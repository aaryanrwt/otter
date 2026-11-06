# Otter CLI verification report

This document records the results of physical download tests, multi-download concurrency, process interruption recovery, and artifact inspection on Otter v0.1.0.

- Date: 2026-11-06
- Environment: Windows 11 (x86_64), Rust 1.96.1 (2024 edition)
- Binary tested: `target\release\otter.exe`

---

## Real-world download test results

All tests downloaded real files from public endpoints, Hugging Face repositories, and Ollama registries. Checksums were verified with SHA-256:

| Test ID | Resource description | URL or source | File size | Time | Average throughput | Verified SHA-256 |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| DL-50M | Cloudflare 50MB binary | `speed.cloudflare.com` | 50.0 MB | 16.31 s | 3.07 MB/s | `fc733e3aa0f1fa7566166a7a70d623686a817aa470db6bc05e231a53781fdb63` |
| DL-45M | Hacker News Parquet (2024-03) | Hugging Face Dataset | 44.8 MB | 16.88 s | 2.65 MB/s | `321679296ad7dad160d9b1f41388827dc46a55396657e7b1e3c6b0f7c99b4980` |
| DL-HF-CFG | BERT Base model configuration | Hugging Face Model | 570 B | 1.29 s | 0.44 KB/s | `7160e1553ad2ca51d8c1cb066be533db31826e12d173824c1bb0cb1a4f187d20` |
| DL-MULTI-1 | Concurrent stream 1 (Parquet) | Hugging Face Dataset | 44.8 MB | 44.90 s | 1.00 MB/s | `321679296ad7dad160d9b1f41388827dc46a55396657e7b1e3c6b0f7c99b4980` |
| DL-MULTI-2 | Concurrent stream 2 (Parquet) | Hugging Face Dataset | 43.6 MB | 36.50 s | 1.17 MB/s | `6ee8c07d985e17872d85c6b206727020fad70d776e86dc830c046553bcdf37bb` |
| DL-MULTI-3 | Concurrent stream 3 (Binary) | `speed.cloudflare.com` | 50.0 MB | 39.48 s | 1.27 MB/s | `fc733e3aa0f1fa7566166a7a70d623686a817aa470db6bc05e231a53781fdb63` |

---

## Concurrent multi-download verification

We launched three separate `otter download` jobs at the same time in independent processes:
- Process 1 (44.8 MB Parquet): finished in 44.90 seconds.
- Process 2 (43.6 MB Parquet): finished in 36.50 seconds.
- Process 3 (50.0 MB Binary): finished in 39.48 seconds.
- Total wall-clock time for all three jobs: 44.99 seconds.
- Hash checks: all three completed files matched their expected SHA-256 hashes without corruption or data collision between processes.

---

## Interruption and resumption verification

- Interruption procedure: a 44.8 MB download was terminated with SIGTERM after two seconds of transfer.
- Disk verification: the partial file `interrupted.parquet.part` remained on disk with 46,960,585 bytes recorded in the preallocated space.
- Resumption: executed `otter resume 18e54a07`.
- Result: the engine queried completed byte ranges from the SQLite WAL, resumed fetching from the remaining byte offsets, and finalized the file.
- Checksum: `otter verify` confirmed the final file matched SHA-256 `321679296ad7dad160d9b1f41388827dc46a55396657e7b1e3c6b0f7c99b4980`.

---

## Cancellation and file cleanup

- Test procedure: initiated a 50 MB download, paused after one second, and confirmed the `.part` file was present on disk.
- Execution: ran `otter cancel 18e54a15`.
- Result: the job was marked as cancelled in the SQLite database and the temporary `.part` file was unlinked from disk immediately.

---

## Artifact inspection verification

We verified that the artifact inspection subsystem parses all four target formats directly in pure Rust:
1. Parquet: validated the `PAR1` magic header and footer on `hn_2024_03.parquet` (44.8 MB).
2. Safetensors: parsed 206 tensor names, shapes, and metadata keys on `model.safetensors` (420.0 MB).
3. GGUF: parsed 398 tensors, model architecture, and 13 key-value metadata attributes from an Ollama model file.
4. ONNX: parsed graph nodes, inputs, outputs, and opset imports through the streaming Protobuf decoder.

---

## Windows NTFS Mark-of-the-Web verification

We inspected the alternate data stream on a completed test file (`test_50mb.bin`):

```powershell
Get-Content 'test_50mb.bin' -Stream Zone.Identifier
```

Output confirmed:
```ini
[ZoneTransfer]
ZoneId=3
HostUrl=https://speed.cloudflare.com/__down?bytes=52428800
```
