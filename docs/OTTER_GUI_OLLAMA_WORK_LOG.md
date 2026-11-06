# Otter GUI: Ollama work log

This document records the queries and tasks delegated to local Ollama models during the architecture, design, and implementation of the Otter v0.1.0 desktop GUI.

- Local runtime: Ollama v0.40.15 on `http://localhost:11434`
- Installed models: `qwen3:4b` (2.5 GB), `qwen2.5-coder:7b` (4.7 GB)

---

## Log of delegated tasks

### Session 1: URL detection and security architecture
- Date: 2026-11-06
- Model: `qwen3:4b`
- Task: URL source detection pattern, SSRF prevention rules, and Tokio DownloadManager to GUI event bridging.
- Query: Technical summary for domain parsing, path traversal checks, and asynchronous event channels between Tokio and GUI thread.
- Accepted output: Decoupled Tokio MPSC/broadcast progress channel for GUI consumption, strict host validation rejecting loopback and private subnets, and normalized path checks blocking traversal (`../`).
