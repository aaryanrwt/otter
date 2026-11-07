# OTTER v0.1.0 — GUI Integration Report
**Document Version:** 1.0.0  
**Status:** Integrated & Verified  
**Engine:** Rust Universal Engine (`crates/engine`, `crates/types`, `crates/gui`)

---

## 1. Engine Integration Verification

The OTTER GUI interfaces directly with `crates/engine` without replacing or bypassing any CLI capabilities.

### 1.1 Direct File Downloads
When a URL is submitted that points to a binary file, dataset, archive, executable, or media container:
- Probing is executed asynchronously via `engine::probe::probe_url(&client, &url)`.
- HTTP Range headers (`bytes=0-0`) verify server range support, `ETag`, `Last-Modified`, and `Content-Length`.
- Dynamic connection scaling allocates 1 stream for files <5MB, up to 4 streams for <30MB, and up to the configured limit (up to 16) for larger transfers.
- Downloads are executed via `DownloadManager::run_job()`, utilizing positioned disk writes and chunk verification.

### 1.2 Universal Stream Media Extraction (`crates/gui/scripts/media_engine.py`)
For streaming video and audio platforms (YouTube, Instagram, Pinterest, TikTok, etc.):
- URLs are probed via Python `yt-dlp` using embedded system node and ffmpeg runtimes.
- Probed metadata returns authentic title, clean filename, duration, thumbnail, and the exact resolutions provided by the source.
- Resolution filters enforce reality: if a video only has 240p or 480p, higher brackets (720p, 1080p, 4K) are strictly omitted from the dropdown.
- Downloading streams runs with `'noprogress': True` and outputs line-delimited JSON progress objects.
- Fast remuxing into standard MP4 is performed with `merge_output_format: 'mp4'`.
- MP3/M4A audio extraction is performed with `FFmpegExtractAudio`.

### 1.3 Cancellation & Recovery
- When the user cancels a download from the UI (`[x]` button), `cancel_download` triggers the corresponding `tokio_util::sync::CancellationToken`.
- Running subprocesses (for media) or async connection pools (for direct engine) are terminated immediately.
- Partial `.part` files are removed from the filesystem.
- The record is purged from the SQLite manifest and active memory snapshot, followed by an immediate state broadcast to the UI.

---

## 2. Dynamic Path Resolution & OS Hardening
- **Path Invalidation Protection:** Default paths configured on non-existent drive letters (such as `D:\`) automatically fall back to `%USERPROFILE%\Downloads\Otter`.
- **Directory Auto-Creation:** Parent directories are verified and created with `std::fs::create_dir_all`.
- **Process Mutex:** Windows named mutex `Local\OtterUniversalDownloadManager_Mutex` guarantees single-instance execution.
