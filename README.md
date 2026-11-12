# RustView

<img src="assets/rustview-icon.png" width="112" height="112" alt="RustView's official lotus icon">

[![Windows x64 CI](https://github.com/walker1911/RustView/actions/workflows/ci.yml/badge.svg)](https://github.com/walker1911/RustView/actions/workflows/ci.yml)

**Open the archive. Keep the flow.**

An archive-first photo viewer for **Windows x64**, written in Rust. Familiar keys, a Picasa-style filmstrip, smooth zoom, camera RAW, Photoshop composites, and pictures inside compressed archives.

[Download v0.6.3](https://github.com/walker1911/RustView/releases/tag/v0.6.3) · [简体中文](README.zh-CN.md) · [Compatibility](docs/COMPATIBILITY.md) · [Build guide](BUILDING.md)

![RustView's native Windows viewer with its graphite theme and Picasa-style filmstrip](assets/readme/viewer.png)

**v0.6.3** brings a graphite interface to Manage and View, with grouped controls and softer selection colors. Hold `←` or `→` to browse local JPEGs with quick target previews. Release the key to finish loading the selected image at full quality. [v0.6.3 release notes](docs/RELEASE-v0.6.3.md) · [Archive resume and jump](docs/RELEASE-v0.6.0.md)

## Start with the pictures

| Capability | What you can do |
|---|---|
| **Browse archives** | Read ZIP / RAR / 7z images on demand with password prompts, resume from the last image shown, and jump by filename, path, or number. |
| **Keep browsing** | Left/right changes images and up/down zooms. Hold an arrow key for quick local JPEG previews. A flat, animated filmstrip keeps the selected photo centered, with a filename and `1/84` counter. |
| **View modern formats** | Common images, LibRaw-supported RAW and ProRAW DNG, plus HEIC/HEIF, AVIF, and JPEG XL. |
| **Open Photoshop documents** | View saved PSD/PSB composites locally or inside ZIP/RAR/7z, including sources larger than 512 MiB. |
| **Zoom and compare** | Pointer-anchored zoom, panning, visible detail tiles, and up to four linked or independent comparison views. |
| **Play and export animation** | Stream GIF/WebP with timing, disposal, and loop handling; export complete animations. |
| **Work from the source** | RAW adjustments, crop, resize, color, sharpening, text/watermarks, real 16-bit PNG/TIFF, processing records, and presets. |
| **Manage a collection** | Rename previews, folder/XMP moves, ratings, labels, explicit EXIF/IPTC/XMP editing, and contact sheets. |

![RustView's native Windows management grid with a selected image and its preview](assets/readme/management.png)

![Archive browsing flow](assets/readme/archive-flow.svg)

In single-image view, click **跳页…** to find an archive image. Enter part of a filename, the full path inside the archive, or a number starting at 1 in the current browsing order. When filenames repeat, use the path to select the intended entry. Resume positions are stored in the local catalog. Encrypted archives prompt for the password again when reopened.

During rapid navigation, RustView can reuse a matching read in progress. While the target preview loads, the previous image's clear overview stays visible with a label identifying both images. Opening an individual image starts the read before directory scanning. For a static image opened at launch, the supervised read can also overlap graphics initialization. [Browsing behavior](docs/PICASA-BROWSING.md)

ZIP and 7z updates create a backup. RAR is read-only. RAW metadata uses XMP sidecars.

## Memory has a budget

Decoding uses a separate, memory-limited Windows process tree. Failed records can be bypassed to view the original.

| Resource | Limit |
|---|---:|
| Recent still-image cache for the current folder or archive | 0–2048 MiB, adjustable in Options |
| GPU detail tiles | 256 MiB |
| Image worker process tree | 2 GiB |
| Ordinary input / static RGBA output | 512 MiB each |
| Validated PSD/PSB source, local or archived | 64 GiB; pixel budgets still apply |
| Animation | Current frame + one prefetched frame |

Press `Alt+O` to adjust the recent still-image cache. The initial limit is based on your computer's physical memory. Set it to 0 to disable the cache, or select the recommended value to restore it. Save options to keep the setting across launches.

![Memory and worker architecture](assets/readme/memory.svg)

**Subsystem budgets** exclude GUI, driver, and preview overhead. Oversized inputs may be refused. [Validation details](docs/VALIDATION.md).

v0.4.0 permits [local and ZIP/RAR/7z-contained PSD/PSB](docs/PSD.md) sources up to 64 GiB. It seeks directly to local composites or streams only the selected archive entry to temporary disk and cleans it after the task. RGBA and process-memory budgets remain unchanged. Very large composites can still exceed the pixel budget. Archive extraction needs space for the selected entry plus a 2 GiB reserve; solid archives may take longer to reach it.

## Download and use

1. Download the complete Windows x64 package. [Offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.6.3/RustView-0.6.3-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.6.3/RustView-0.6.3-win-x64-offline.zip.sha256)
2. Fully extract it and double-click `RustView.exe` or `Start-RustView.cmd`. The package includes the image engines and runtime libraries for offline use from first launch.
3. Open or drag in a folder, picture, or archive. Keep the complete extracted folder.

The offline ZIP is the only download needed for viewing. The source guide lists the archives for rebuilding the bundled engines. [Offline guide](OFFLINE.md) · [Native sources](docs/NATIVE-SOURCES.md)

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
pwsh -File .\tools\build.ps1 -Offline
```

See [BUILDING.md](BUILDING.md) for checksums and validation steps.

Release checks cover static images and archives, GIF/WebP animation, PSD/PSB composites, thumbnails, and large encrypted-RAR cancellation. Archive resume and jump were tested with the viewer's interface logic and real Windows decoding workers in a headless harness. [v0.6.0 release notes](docs/RELEASE-v0.6.0.md) · [Validation results and scope](docs/VALIDATION.md)

Real-camera samples include Nikon Z6, Sony a7 III, Canon EOS R, iPhone 12 Pro ProRAW and HEIC. See the validation record for the scope of native-window, display, and print checks.

[Report an issue](https://github.com/walker1911/RustView/issues) with the version, format/camera, expected behavior, and a minimal reproduction.

## Scope and license

RustView's scope excludes split and self-extracting archives, RAR writing, and imports of ACDSee's private database and development records. Unconfirmed legacy commands stay inactive. Slideshows, calendars, disc burning, online services, and audio/video playback are outside the project scope. [Compatibility limits](docs/COMPATIBILITY.md)

RustView is independent of ACD Systems. Application code is [MIT licensed](LICENSE); reused egui/wgpu, image, LibRaw, 7-Zip, ImageMagick, and ExifTool components retain their licenses. [Third-party notices](THIRD-PARTY-NOTICES.md).
