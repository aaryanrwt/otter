# OTTER v0.1.0 — Settings Known Limitations

1. **System Tray Integration on Minimize:**
   - Minimizing to system tray requires a notification icon loop registered with `Shell_NotifyIconW`. While the preference toggle is persisted and respected by the window close event, full multi-option tray context menus (e.g. Pause All, Resume All from taskbar tray) will be enhanced in v0.2.0.

2. **Language Localization Coverage:**
   - While the UI selector supports English, Spanish, German, French, and Japanese, translation dictionaries are currently populated with English strings as the master baseline. Future releases will load external `.ftl` (Fluent) localization files.

3. **Folder Browser Dialog Threading:**
   - On Windows, `FolderBrowserDialog` runs asynchronously inside a background PowerShell worker to prevent blocking the WebView2 UI event thread. On slower hard drives or heavily loaded systems, opening the dialog may take 300–500ms before appearing.
