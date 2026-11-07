# OTTER v0.1.0 — Settings Release Checklist

- [x] Engine page completely removed from sidebar navigation and main viewport.
- [x] Settings page two-column layout matches reference screenshot (`media_1793981035328_bf7d5ee6.jpg`).
- [x] Top-right sleeping otter mascot on pink paw pillow rendered cleanly via Base64 inlining with animated floating "z z z" typography.
- [x] 8 secondary subnav categories with vector SVG brand icons (General, Detection, Download Engine, Storage, Network, Appearance, Notifications, Advanced).
- [x] Two distinct white rounded cards for General settings (`General` and `Download Behavior`).
- [x] "Start with Windows" toggle hooked up to Windows Registry `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
- [x] "Minimize to system tray" toggle hooked up and persisted.
- [x] "Language" pill dropdown selector wired and persisted.
- [x] "Default download location" displays folder icon, active path, and native Windows `Change` folder dialog button.
- [x] "Concurrent downloads" pill dropdown (2, 3, 4, 5, 8, 16) wired and persisted.
- [x] "Bandwidth limit" pill dropdown (No limit, 5, 10, 25, 50, 100 MB/s) wired and persisted.
- [x] Zero mock or predefined user data; settings stored atomically in `%LOCALAPPDATA%\otter\gui_settings.json`.
- [x] Complete workspace test suite passes with zero failures (`cargo test --workspace`).
