# OTTER v0.1.0 — GUI Bug Ledger & Resolution Log
**Document Version:** 1.0.0  
**Status:** All Critical Defects Resolved & Tested

---

## 1. Defect Ledger

| Bug ID | Severity | Component | Description | Root Cause | Resolution | Verification |
|---|---|---|---|---|---|---|
| **BUG-01** | **CRITICAL** | `media_engine.py` / `main.rs` | Media downloads failing immediately (`WinError 3`). | Default download directory was hardcoded to `D:\Downloads\Otter` on a system with only drive `C:`. | Replaced with dynamic resolution to `%USERPROFILE%\Downloads\Otter`. | Verified with end-to-end download to local drive. |
| **BUG-02** | **CRITICAL** | `media_engine.py` | Progress lines corrupted, Rust parser failed. | `yt-dlp` outputted default carriage-return progress text to stdout. | Added `'noprogress': True` and updated Rust to find `{"type":`. | Verified 100% clean JSON stream. |
| **BUG-03** | **HIGH** | `main.rs` | YouTube URLs treated as direct HTTP HTML downloads. | Clicking Start Download before probe finished sent `is_media: false`. | Enforced `is_media_url()` in Rust to always route media URLs to the media engine. | Verified YouTube URLs never fall back to direct GET. |
| **BUG-04** | **HIGH** | `main.rs` | Multiple instance collision crash (`HRESULT 0x800700AA`). | Launching a second instance locked WebView2 user data folder. | Implemented Win32 `SingleInstanceGuard` mutex to bring existing window to front. | Verified duplicate launch exits cleanly without error. |
| **BUG-05** | **MEDIUM** | `app.js` | YouTube downloads showed folder icon. | `getBrandIcon()` was missing a YouTube domain handler. | Added SVG YouTube red play badge. | Verified YouTube rows show red play icon. |
| **BUG-06** | **MEDIUM** | `app.js` / `index.html` | First run user greeting hardcoded to "User" or prior name. | No initial onboarding popup. | Implemented first-run onboarding modal that prompts on empty username. | Tested dynamic name entry and persistence. |
| **BUG-07** | **MEDIUM** | `style.css` | Search icon was generic grey. | Generic SVG styling. | Restyled with capsule pill, `#F472B6` stroke, and pink focus glow. | Verified matching Otter design vibe. |
| **BUG-08** | **MEDIUM** | `media_engine.py` | Video merging was slow or timing out. | Used `FFmpegVideoConvertor` which re-encoded the video. | Changed to `merge_output_format: 'mp4'` for fast remuxing. | 8.99 MB video merged in <20s. |
