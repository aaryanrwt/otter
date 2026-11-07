# OTTER v0.1.0 — Settings Implementation Report

## Overview
This report details the implementation of the OTTER v0.1.0 Settings Page and the architectural cleanup requested to match the target reference design (`media_1793981035328_bf7d5ee6.jpg`).

## Completed Architectural Changes

### 1. Removal of Dedicated Engine View
- The 5th item `Engine` in the primary sidebar navigation was removed.
- The corresponding `<section class="view" id="view-engine">` telemetry dashboard was completely eliminated from `crates/gui/ui/index.html`.
- Telemetry render loops and diagnostic event listeners (`setupEngine`, `renderEngine`) were stripped from `crates/gui/ui/app.js` to ensure zero dead code or orphaned DOM query references.
- Concurrency, rate limiting, and disk engine settings have been cleanly unified into the `Download Engine` sub-pane within the Settings view.

### 2. Settings Page Composition & Visual Fidelity
The Settings page now matches the reference screenshot in structure and styling:
- **Header:**
  - Distinct title `Settings .` with the signature pink dot.
  - Subtitle: `Customize how Otter works for you.`
  - Top-right mascot: Sleeping Otter resting on the pink paw pillow (`Sleeping Otter with Pink Paw Pillow.png`) accompanied by staggered, floating "z z z" typography with CSS micro-animations.
- **Two-Column Split Layout:**
  - **Left Subnav (8 Categories):**
    1. `General` (Gear vector SVG) — Active state styled with a soft rose pill background (`#FDE8EE`).
    2. `Detection` (Wifi/broadcast vector SVG)
    3. `Download Engine` (Lightning bolt vector SVG)
    4. `Storage` (Drawer/disk vector SVG)
    5. `Network` (Globe vector SVG)
    6. `Appearance` (Palette vector SVG)
    7. `Notifications` (Bell vector SVG)
    8. `Advanced` (Code brackets `<>` vector SVG)
  - **Right Content Column (General Pane):**
    - **Card 1 (`General`):**
      - `Start with Windows`: Linked to native Windows registry autostart (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`), controlled via a pink toggle switch.
      - `Minimize to system tray`: Controlled via a pink toggle switch.
      - `Language`: Native rounded pill dropdown selector (`English ⌵`).
    - **Card 2 (`Download Behavior`):**
      - `Default download location`: Displays an outline folder icon, current target directory (`D:\Downloads\Otter`), and an outline pill button `Change`. Clicking `Change` invokes a native Windows `FolderBrowserDialog` asynchronously.
      - `Concurrent downloads`: Pill dropdown (`5 ⌵`).
      - `Bandwidth limit`: Pill dropdown (`No limit ⌵`).

### 3. Native Windows & State Integration
- **Storage:** Settings are stored in `%LOCALAPPDATA%\otter\gui_settings.json` with atomic file persistence (`.tmp` write followed by `std::fs::rename`).
- **Autostart:** Uses `reg.exe` with `CREATE_NO_WINDOW` (`0x08000000`) to avoid console flashing when toggling Windows autostart.
- **Folder Picker:** Spawns `powershell` with `System.Windows.Forms.FolderBrowserDialog` on a worker thread and dispatches the updated path back to the Webview via `EventLoopProxy<UserEvent>`.

## Verification Status
- `cargo check -p gui`: Clean, 0 errors, 0 warnings.
- `cargo build -p gui --bin otter-gui`: Succeeded in `target\debug\otter-gui.exe`.
- `cargo test --workspace`: 100% test pass rate across engine, harness, types, and cli.
