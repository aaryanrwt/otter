# Otter GUI known limitations

Document version: 1.0.0
Engine version: 0.1.0
Author: aaryanrwt

## 1. Current platform boundaries

1. Windows WebView2 prerequisite:
   - Otter GUI requires the Microsoft Edge WebView2 runtime. On Windows 10 (updated) and Windows 11, this runtime is preinstalled as a core OS component. On bare Windows Server installations or un-updated LTSC builds, the Evergreen WebView2 bootstrapper must be installed.

2. Media platform extraction limits:
   - For direct YouTube, Instagram, and Pinterest video URLs, Otter provides stream parsing and direct downloading. However, if the hosting platform requires authenticated session cookies or challenges the client with dynamic BotGuard / CAPTCHA tokens, direct extraction will fail gracefully and surface an informative error card rather than crashing.

3. Background system tray minimization:
   - The desktop client provides standard window controls (minimize, maximize, close). Deep system tray minimization with background icon docking will be expanded in v0.2.0.

4. Win32 file preallocation privileges:
   - Win32 `SetFileValidData` (instant allocation of multi-gigabyte files without zero-filling disk sectors) requires the `SE_MANAGE_VOLUME_NAME` privilege. When the application runs without elevated administrative rights, the engine automatically falls back to standard sparse allocation via `SetEndOfFile`, ensuring safe operation across all standard user accounts.
