# RustView

[![Windows x64 CI](https://github.com/walker1911/RustView/actions/workflows/ci.yml/badge.svg)](https://github.com/walker1911/RustView/actions/workflows/ci.yml)

**Open the archive. Keep the flow.**

An archive-first photo viewer for **Windows x64**, written in Rust. Familiar keys, a Picasa-style filmstrip, smooth zoom, camera RAW, Photoshop composites, and pictures inside compressed archives.

[Download v0.4.0](https://github.com/walker1911/RustView/releases/tag/v0.4.0) · [简体中文](README.zh-CN.md) · [Compatibility](docs/COMPATIBILITY.md) · [Build guide](BUILDING.md)

![RustView illustrated overview](assets/readme/hero.svg)

*Project illustration, not an application screenshot. v0.4.0 is an early public prerelease.*

## Start with the pictures

| Capability | What you can do |
|---|---|
| **Browse archives** | Read ZIP / RAR / 7z images on demand, with password prompts; leave the rest of the archive packed. |
| **Keep browsing** | Left/right changes images; up/down zooms. A flat, animated filmstrip keeps the selected photo centered, with a filename and `1/84` counter. |
| **View modern formats** | Common images, LibRaw-supported RAW and ProRAW DNG, plus HEIC/HEIF, AVIF, and JPEG XL. |
| **Open Photoshop documents** | View saved PSD/PSB composites locally or inside ZIP/RAR/7z, including sources larger than 512 MiB. |
| **Zoom and compare** | Pointer-anchored zoom, panning, visible detail tiles, and up to four linked or independent comparison views. |
| **Play and export animation** | Stream GIF/WebP with timing, disposal, and loop handling; export complete animations. |
| **Work from the source** | RAW adjustments, crop, resize, color, sharpening, text/watermarks, real 16-bit PNG/TIFF, processing records, and presets. |
| **Manage a collection** | Rename previews, folder/XMP moves, ratings, labels, explicit EXIF/IPTC/XMP editing, and contact sheets. |

![Archive browsing flow](assets/readme/archive-flow.svg)

Session-only passwords; backed-up ZIP/7z updates; read-only RAR. Catalog data remains separate; RAW metadata uses XMP sidecars.

During rapid navigation, a cached preview of the target appears first when available. Otherwise a tiny mosaic of the previous image stays visible, labeled with the pending target, until the decoded image arrives. After an image has been shown, loading, password prompts and errors keep this continuity preview instead of replacing it with the welcome screen. [Browsing behavior](docs/PICASA-BROWSING.md).

## Memory has a budget

Decoding uses a separate, memory-limited Windows process tree. Failed records can be bypassed to view the original.

| Resource | Limit |
|---|---:|
| Image LRU cache | 768 MiB |
| GPU detail tiles | 256 MiB |
| Image worker process tree | 2 GiB |
| Ordinary input / static RGBA output | 512 MiB each |
| Validated PSD/PSB source, local or archived | 64 GiB; pixel budgets still apply |
| Animation | Current frame + one prefetched frame |

![Memory and worker architecture](assets/readme/memory.svg)

**Subsystem budgets** exclude GUI, driver, and preview overhead. Oversized inputs may be refused. [Validation details](docs/VALIDATION.md).

v0.4.0 permits [local and ZIP/RAR/7z-contained PSD/PSB](docs/PSD.md) sources up to 64 GiB. It seeks directly to local composites or streams only the selected archive entry to temporary disk and cleans it after the task. RGBA and process-memory budgets remain unchanged. Very large composites can still exceed the pixel budget. Archive extraction needs space for the selected entry plus a 2 GiB reserve; solid archives may take longer to reach it.

## Download and use

1. Download the [complete Windows x64 offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.4.0/RustView-0.4.0-win-x64-offline.zip).
2. Fully extract it and double-click `RustView.exe` or `Start-RustView.cmd`. All image engines and runtime libraries are included; first launch needs no internet access or installation.
3. Open or drag in a folder, picture, or archive. Keep the complete extracted folder.

The ZIP has an accompanying `.sha256` file. Matching third-party source attachments are provided on the same Release and are not needed for normal offline viewing. [Offline guide](OFFLINE.md) · [What changed in v0.4.0](docs/RELEASE-v0.4.0.md).

The interface is currently primarily Chinese; a complete English UI is not yet available. See the [full keyboard reference](docs/KEYBOARD.md).

Data stays in `RustView-data` beside the executable. Ordinary viewing needs no PATH or registry changes. Optionally run `Setup-File-Associations.cmd` to register image candidates and an **Open with RustView** context-menu entry for ZIP/RAR/7z. Windows Default Apps confirms image defaults; archive defaults remain unchanged. [Windows integration](docs/WINDOWS-INTEGRATION.md).

### A few familiar keys

Mode-aware bindings distinguish numpad keys and support customization with recognized JSON/REG imports. View arrows do not take over text fields or the management grid. An imported complete legacy shortcut set may need the new arrow bindings added in shortcut settings.

| Action | Default keys |
|---|---|
| Open | `Ctrl+O` |
| Enter View / return to Manage | `Enter` / `Enter`, `Esc`, `Ctrl+W` |
| Previous / next image | `←` / `→`; `PageUp` / `PageDown`; `Backspace` / `Space` |
| Smooth zoom in / out | `↑` / `↓` |
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

The v0.4.0 unit suite passes **208 tests**, with **2 public-camera tests ignored by default**. The release checks cover static images/archives, GIF/WebP animation, PSD/PSB composites, thumbnail reads, encrypted large-RAR cancellation and same-Viewer rapid navigation. See the [release notes](docs/RELEASE-v0.4.0.md) and [validation record](docs/VALIDATION.md) for exact results and limits.

Real-camera samples include Nikon Z6, Sony a7 III, Canon EOS R, iPhone 12 Pro ProRAW and HEIC; this does not certify every model or encoding. Native-window password retry, navigation and close were checked through UI automation. Screenshot capture remains unavailable, so full visual, DPI, multi-monitor and paper-printing acceptance remains incomplete.

[Report an issue](https://github.com/walker1911/RustView/issues) with the version, format/camera, expected behavior, and a minimal reproduction.

## Scope and license

v0.4.0 has [documented compatibility limits](docs/COMPATIBILITY.md): no split or self-extracting archives, RAR writing, or import of ACDSee's private database/development records. Unconfirmed legacy commands stay inactive. Slideshows, calendars, disc burning, online services, and audio/video playback are outside the project scope.

RustView is independent of ACD Systems. Application code is [MIT licensed](LICENSE); reused egui/wgpu, image, LibRaw, 7-Zip, ImageMagick, and ExifTool components retain their licenses. [Third-party notices](THIRD-PARTY-NOTICES.md).
