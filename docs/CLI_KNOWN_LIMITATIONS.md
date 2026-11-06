# Otter CLI known limitations

This document outlines environment-specific constraints and upstream service limitations observed during testing of Otter v0.1.0.

## Storage and filesystem constraints

- NTFS alternate data streams: Mark-of-the-Web (`:Zone.Identifier`) metadata is specific to Windows NTFS volumes. On FAT32, exFAT, or non-Windows filesystems, the ADS write operation logs an informational trace and skips the stream without failing the download.
- Preallocation on network storage: While NTFS on local NVMe or SATA SSDs allocates disk space instantly, some network-attached shares (SMB or CIFS) or non-NTFS volumes write physical zeros synchronously during preallocation. This can add a few seconds of initial setup time for multi-gigabyte models.

## Remote CDN and server constraints

- Cloudflare speed test limits: The `speed.cloudflare.com/__down?bytes=` testing endpoint caps individual slice sizes at 50 MB. Single range requests larger than 50 MB return HTTP 403 Forbidden.
- Hugging Face unauthenticated queries: Unauthenticated anonymous requests to `huggingface.co/api/models/...` can trigger intermittent HTTP 429 rate limits when making rapid bursts of metadata calls. Setting the `HF_TOKEN` environment variable prevents this.

## Terminal environments

- Color support: The terminal dashboard uses 24-bit TrueColor. It is tested on Windows Terminal, PowerShell 7+, and the VS Code integrated terminal. Legacy `conhost.exe` (classic Windows Command Prompt) displays the closest 16-color ANSI match.
- Recommended terminal width: A width of at least 105 columns provides the best layout for the side-by-side view. On narrower screens, the dashboard collapses into a vertical single-column view so all text remains visible.
