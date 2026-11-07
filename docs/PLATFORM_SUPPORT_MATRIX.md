# OTTER v0.1.0 — Platform Support Matrix
**Document Version:** 1.0.0  
**Status:** Verified  
**Engine:** Rust Universal Engine (`crates/engine`, `crates/gui`)

---

## 1. Supported Platform Matrix

| Category | Platform / Service | Detection | Management | Acceleration | Authentic Logo | Tested Verification Status |
|---|---|---|---|---|---|---|
| **Video Streams** | YouTube | Automatic URL / Process | Media Engine | Multi-stream remux | Vector Red Play Badge | **PASS** (4K, 1080p, 720p, 480p, 240p, MP3) |
| **Video Streams** | Instagram (Reels & Posts) | Automatic URL / Process | Media Engine | Direct MP4 stream | Authentic Instagram Logo | **PASS** (MP4 Video & MP3 Audio) |
| **Video Streams** | Pinterest (Pins & Videos) | Automatic URL / Process | Media Engine | Direct MP4 stream | Authentic Pinterest Logo | **PASS** (Web & MP4 extraction) |
| **Video Streams** | TikTok | Automatic URL | Media Engine | Direct MP4 stream | Neutral Media Fallback | **PASS** (Tested via probe extractor) |
| **Video Streams** | Twitter / X | Automatic URL | Media Engine | Direct MP4 stream | Neutral Media Fallback | **PASS** (Video clips extracted) |
| **AI / ML** | Hugging Face (Models & Datasets) | URL / safetensors / GGUF | Native Rust Engine | AIMD Multi-Connection | Authentic Hugging Face Logo | **PASS** (Inspected safetensors & parquet) |
| **AI / ML** | Ollama | CLI / Tag format | Native Rust Engine | Blob multi-stream | Official Ollama Mascot / Logo | **PASS** (Tested manifest & blob lifecycle) |
| **AI / ML** | Kaggle | Datasets / Models | Native Rust Engine | Chunked transfer | Neutral AI Fallback | **PASS** (Public dataset archive URLs) |
| **Game Launchers** | Steam | Windows Process Telemetry | Detected Only | Launcher-controlled | Authentic Steam Logo | **PASS** (Detects `steam.exe`, reports active) |
| **Game Launchers** | Epic Games Launcher | Windows Process Telemetry | Detected Only | Launcher-controlled | Authentic Epic Games Logo | **PASS** (Detects `EpicGamesLauncher.exe`) |
| **Web Browsers** | Google Chrome | Windows Process & FS hook | Interception / Handoff | Native Rust Engine | Authentic Google Chrome Logo | **PASS** (Monitors `chrome.exe` & `.crdownload`) |
| **Web Browsers** | Microsoft Edge | Windows Process & FS hook | Interception / Handoff | Native Rust Engine | Authentic Edge Logo | **PASS** (Monitors `msedge.exe` & `.crdownload`) |
| **Developer Tools** | GitHub Releases | Direct URL / Releases | Native Rust Engine | Multi-Connection Range | Authentic GitHub Logo | **PASS** (Releases, tarballs, ZIPs) |
| **Direct Web** | HTTP / HTTPS Direct Links | Universal URL Detector | Native Rust Engine | AIMD Multi-Connection | Generic Folder Icon | **PASS** (Range-supported binary streams) |

---

## 2. Integrity & Honesty Rules
1. **Steam & Epic Games:** Never claimed as "accelerated" unless OTTER is executing the byte stream. Because Steam and Epic use proprietary encrypted chunk protocols, OTTER displays their real process status as `Active` or `Detected`, with aggregate bandwidth when telemetry allows.
2. **Dynamic Media Qualities:** If a YouTube or Instagram video only exists in 240p, 1080p will NEVER appear in the quality selector.
3. **DRM & Protected Media:** OTTER strictly honors copyright, paywalls, and Widevine/PlayReady restrictions, reporting `Unsupported` or `Authentication Required` without attempting to compromise client security boundaries.
