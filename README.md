# RustView

[![Windows x64 CI](https://github.com/walker1911/RustView/actions/workflows/ci.yml/badge.svg)](https://github.com/walker1911/RustView/actions/workflows/ci.yml)

**Open the archive. Keep the flow.**

An archive-first photo viewer for **Windows x64**, written in Rust. Familiar keys, smooth zoom, camera RAW, and pictures inside compressed archives.

[Download v0.3.0](https://github.com/walker1911/RustView/releases/tag/v0.3.0) · [简体中文](README.zh-CN.md) · [Compatibility](docs/COMPATIBILITY.md) · [Build guide](BUILDING.md)

![RustView illustrated overview](assets/readme/hero.svg)

*Project illustration, not an application screenshot. v0.3 is an early public release.*

## Start with the pictures

| Capability | What you can do |
|---|---|
| **Browse archives** | Read ZIP / RAR / 7z images on demand, with password prompts; leave the rest of the archive packed. |
| **View modern formats** | Common images, LibRaw-supported RAW and ProRAW DNG, plus HEIC/HEIF, AVIF, and JPEG XL. |
| **Zoom and compare** | Pointer-anchored zoom, panning, visible detail tiles, and up to four linked or independent comparison views. |
| **Play and export animation** | Stream GIF/WebP with timing, disposal, and loop handling; export complete animations. |
| **Work from the source** | RAW adjustments, crop, resize, color, sharpening, text/watermarks, real 16-bit PNG/TIFF, processing records, and presets. |
| **Manage a collection** | Rename previews, folder/XMP moves, ratings, labels, explicit EXIF/IPTC/XMP editing, and contact sheets. |

![Archive browsing flow](assets/readme/archive-flow.svg)

Session-only passwords; backed-up ZIP/7z updates; read-only RAR. Catalog data remains separate; RAW metadata uses XMP sidecars.

## Memory has a budget

Decoding uses a separate, memory-limited Windows process tree. Failed records can be bypassed to view the original.

| Resource | Limit |
|---|---:|
| Image LRU cache | 768 MiB |
| GPU detail tiles | 256 MiB |
| Image worker process tree | 2 GiB |
| Input file / static RGBA output | 512 MiB each |
| Animation | Current frame + one prefetched frame |

![Memory and worker architecture](assets/readme/memory.svg)

**Subsystem budgets** exclude GUI, driver, and preview overhead. Oversized inputs may be refused. [Validation details](docs/VALIDATION.md).

## Download and use

1. Download the [Windows x64 bootstrap ZIP](https://github.com/walker1911/RustView/releases/download/v0.3.0/RustView-0.3.0-win-x64-bootstrap.zip).
2. Extract it and run `Start-RustView.cmd`. First setup downloads pinned native engines from official sources and checks their hashes; it needs internet access.
3. After setup, run `RustView.exe` for offline viewing. Open or drag in a folder, picture, or archive.

The interface is currently primarily Chinese; a complete English UI is not yet available. See the [full keyboard reference](docs/KEYBOARD.md).

Data stays in `RustView-data` beside the executable. Setup needs no PATH, registry, or file-association changes.

### A few familiar keys

Mode-aware bindings distinguish numpad keys and support customization with recognized JSON/REG imports.

| Action | Default keys |
|---|---|
| Open | `Ctrl+O` |
| Enter View / return to Manage | `Enter` / `Enter`, `Esc`, `Ctrl+W` |
| Previous / next image | `PageUp` / `PageDown`; `Backspace` / `Space` |
| Zoom / fit / actual pixels | Numpad `+` and `−` / `*` / `/` |
| Pause, resume, or replay GIF/WebP | `G` |
| Rename / compare selection | `F2` / `Ctrl+Alt+R` |
| Properties / shortcut settings | `Alt+Enter` / `Alt+O` |

## Build, test, contribute

Build on Windows x64 with Rust MSVC, C++ Build Tools/Windows SDK, and PowerShell 7. Fetch the pinned engines first:

```powershell
git clone https://github.com/walker1911/RustView.git
cd RustView
pwsh -File .\tools\fetch-native.ps1
cargo build --release --locked
cargo test --locked -- --test-threads=2
pwsh -File .\tools\build.ps1
```

See [BUILDING.md](BUILDING.md) for checksums and validation steps.

Public-source v0.3 validation records **159 unit tests passed** (2 public-sample tests skipped by default), with a **68-case local release baseline**. A [clean Windows-2022 CI run](https://github.com/walker1911/RustView/actions/runs/42846831762) also passed the unit suite, build, and **49 worker cases**: 25 static/archive and 24 animation. The local real-camera samples include Nikon Z6, Sony a7 III, Canon EOS R, iPhone 12 Pro ProRAW, and HEIC—not every model or encoding. Windows denied GUI automation; full native interaction and paper printing remain unverified. [Validation details](docs/VALIDATION.md).

[Report an issue](https://github.com/walker1911/RustView/issues) with the version, format/camera, expected behavior, and a minimal reproduction.

## Scope and license

v0.3 has [documented compatibility limits](docs/COMPATIBILITY.md): no split or self-extracting archives, RAR writing, or import of ACDSee's private database/development records. Unconfirmed legacy commands stay inactive. Slideshows, calendars, disc burning, online services, and audio/video playback are outside the project scope.

RustView is independent of ACD Systems. Application code is [MIT licensed](LICENSE); reused egui/wgpu, image, LibRaw, 7-Zip, ImageMagick, and ExifTool components retain their licenses. [Third-party notices](THIRD-PARTY-NOTICES.md).
