# Otter GUI release readiness

Document version: 1.0.0
Engine version: 0.1.0
Author: aaryanrwt

## 1. Readiness checklist

- [x] Native compilation: `cargo build -p gui --bin otter-gui` produces an unbloated 19 MB Windows binary (`target\debug\otter-gui.exe`).
- [x] Zero mock policy: Hardware specifications, network rates, job lists, and doctor diagnostics query genuine OS and SQLite states.
- [x] Design consistency: Follows the warm ivory `#FAF7F5` aesthetic, card elevation, and visual hierarchy from the four reference screens.
- [x] Mascot assets embedded: All four high-resolution mascot illustrations are embedded via `include_bytes!` and served with transparent alphas.
- [x] SSRF security boundaries: Blocks access to localhost, private IP subnets, and cloud metadata endpoints.
- [x] Multi-crate workspace health: `crates/cli`, `crates/engine`, `crates/types`, and `crates/gui` compile cohesively without cyclic dependencies.
- [x] Documentation complete: Architecture audit, design system, implementation report, test report, feature matrix, known limitations, and release readiness are fully documented in `docs/`.

---

## 2. Binary execution instructions

### Running the desktop GUI:
```powershell
cargo run -p gui --bin otter-gui
```
Or directly executing the compiled executable:
```powershell
.\target\debug\otter-gui.exe
```

### Running the command line interface:
```powershell
cargo run -p cli -- [subcommand]
```
Or directly executing the CLI executable:
```powershell
.\target\debug\otter.exe
```
