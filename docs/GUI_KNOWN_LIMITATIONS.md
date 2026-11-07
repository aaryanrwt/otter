# OTTER v0.1.0 — Desktop GUI Known Limitations
**Document Version:** 1.0.0  
**Status:** Documented & Audited  
**Engine:** Rust Universal Engine (`crates/engine`, `crates/gui`)

---

## 1. Technical & Platform Limitations

### 1.1 Non-Range HTTP Servers
- **Limitation:** If a remote server returns `Accept-Ranges: none` or ignores `Range: bytes=X-Y`, multi-connection acceleration is technically impossible.
- **OTTER Behavior:** OTTER detects this during URL probing, falls back to a single stream with honest indeterminate progress, and disables range resume to avoid file corruption.

### 1.2 Steam & Epic Games Game Downloads
- **Limitation:** Steam (`steam.exe`) and Epic Games (`EpicGamesLauncher.exe`) use proprietary, encrypted, delta-chunked protocols and manage their own file integrity locks.
- **OTTER Behavior:** OTTER detects and monitors active launcher processes and reports their activity honestly in the Activity page without claiming to accelerate their encrypted internal transfers.

### 1.3 Windows Path & Drive Letters
- **Limitation:** Hardcoded drive paths (such as `D:\`) fail on systems without secondary drives (`WinError 3`).
- **OTTER Behavior:** OTTER detects missing drives dynamically and redirects storage to `%USERPROFILE%\Downloads\Otter`.

### 1.4 Protected Media Streams (DRM)
- **Limitation:** Media encrypted with Widevine, FairPlay, or PlayReady DRM (e.g. Netflix, Amazon Prime, Spotify Web) cannot and should not be decrypted by download managers.
- **OTTER Behavior:** OTTER detects DRM tags and reports the content as `Protected / DRM Restricted` without attempting to bypass security boundaries.

### 1.5 Rate-Limited Streaming Services
- **Limitation:** Aggressive probing or multi-connection downloading on platforms like YouTube or Pinterest can trigger temporary IP throttling (HTTP 429).
- **OTTER Behavior:** OTTER uses AIMD backoff, embedded Node.js execution environments, and user-agent rotation to avoid unnecessary throttling.
