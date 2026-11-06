# Otter GUI test report

Document version: 1.0.0
Engine version: 0.1.0
Target: otter-gui (Windows x86_64)
Author: aaryanrwt

## 1. Test execution summary

| Test suite | Target crate | Tests executed | Passed | Failed | Status |
| :--- | :--- | :--- | :--- | :--- | :--- |
| Workspace unit & integration | crates/engine, crates/types | 28 | 28 | 0 | Passed |
| Universal URL detection & SSRF | crates/gui/src/detector.rs | 12 | 12 | 0 | Passed |
| SQLite WAL concurrency | crates/engine/src/manifest.rs | 6 | 6 | 0 | Passed |
| Doctor diagnostic harness | crates/engine/src/doctor.rs | 8 | 8 | 0 | Passed |
| Win32 WebView2 asset delivery | embedded `otter://` handler | 4 | 4 | 0 | Passed |

---

## 2. Universal URL detection test verification

The URL detector was verified across realistic inputs to confirm accurate classification and SSRF containment:

1. Direct HTTP/HTTPS:
   - Input: `https://speed.cloudflare.com/__down?bytes=10485760`
   - Classification: `DirectLink`
   - Content: `Binary / Direct File`
   - Filename: `__down.bin`
   - Result: Passed

2. Hugging Face Safetensors:
   - Input: `https://huggingface.co/bert-base-uncased/resolve/main/model.safetensors`
   - Classification: `HuggingFace`
   - Content: `Model Checkpoint (.safetensors)`
   - Filename: `model.safetensors`
   - Result: Passed

3. Hugging Face Dataset Shard:
   - Input: `https://huggingface.co/datasets/HuggingFaceFW/fineweb/resolve/main/data/train-00014-of-00015.parquet`
   - Classification: `HuggingFace`
   - Content: `Dataset Shard (.parquet)`
   - Filename: `train-00014-of-00015.parquet`
   - Result: Passed

4. Ollama Tag:
   - Input: `ollama:qwen3:4b`
   - Classification: `Ollama`
   - Content: `Ollama Model Tag`
   - Filename: `qwen3_4b.gguf`
   - Result: Passed

5. YouTube Video:
   - Input: `https://www.youtube.com/watch?v=dQw4w9WgXcQ`
   - Classification: `YouTube`
   - Content: `Video / Audio Stream`
   - Result: Passed

6. SSRF Protection:
   - Input: `http://127.0.0.1:8000/secret.txt` -> Rejected (Localhost / Loopback)
   - Input: `http://192.168.1.1/router` -> Rejected (Private network RFC 1918)
   - Input: `http://169.254.169.254/latest/meta-data` -> Rejected (Link-local cloud metadata)
   - Result: Passed

---

## 3. UI responsiveness and mascot asset verification

All four mascot assets compiled cleanly via `include_bytes!` directly into the binary:
1. `Flying Otter Mascot with Flowing Scarf.png`: Validated 1024x1024 PNG with transparent alpha layer.
2. `Focused Otter with Laptop and Coffee.png`: Validated 1024x1024 PNG with transparent alpha layer.
3. `Cheerful Otter Popping Out of Pink Box.png`: Validated 1024x1024 PNG with transparent alpha layer.
4. `Sleeping Otter with Pink Paw Pillow.png`: Validated 1024x1024 PNG with transparent alpha layer.

Under test execution, all four images render without HTTP round-trips or disk reads because they are served directly from binary memory through the registered `otter://images/*` handler.
