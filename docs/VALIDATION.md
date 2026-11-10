# Validation / 验证记录

## v0.3 development baseline

The original Windows x64 development build passed **158 unit tests** with **2 public-camera tests ignored by default**, plus **68 isolated-process regression cases**: 25 static-image/archive, 24 animation, and 19 high-depth/metadata/editing cases. These are development results, not a claim that every GUI interaction is certified. Public-source and bootstrap checks are recorded separately below.

| Area | Coverage |
|---|---|
| Static/archives | Transparent PNG, lossless WebP, 96MP PNG, ZIP/RAR/7z, literal Unicode entry names, ZipCrypto/AES/header encryption, missing/wrong/correct passwords, split-volume refusal and recovery after invalid inputs. |
| Animation | GIF/WebP local and archive reads, composed pixel hashes, disposal/timing/loops, bounded prefetch, cancellation and 1,200 streamed frames. |
| Real samples | Nikon Z6 NEF, Sony a7 III ARW, Canon EOS R CR3, iPhone 12 Pro ProRAW DNG and a libheif HEIC sample: viewing and actual PNG16 + ICC export. These sample photographs are not distributed here. |
| Editing/color | Genuine low bits, output container depths, ICC tag alignment and D50, warning-free native ICC parsing, transparent output, watermarks, animation export timing and processing-record restoration. |
| File/catalog | Real ZIP/7z rename swaps, original backups and encryption, XMP moves, recursive folder operations, catalog-key transactions, persistent clipboard budgets, and prevention of stale/deleted record resurrection. |
| UI logic | Headless virtual layouts for 100,000 entries, mode-preserving refresh, multi-selection, anchored zoom, transparent comparison detail, preview state, delayed RAW cancellation and metadata target binding. |
| Printing | Layout, DPI, fit/fill, page/copy math and bounded 400-cell metadata text; no paper was printed. |
| Recovery | A valid but over-budget saved recipe can be bypassed to display original pixels without deleting the record; animation replay preserves this choice. |

## Public-source release checks

The public source was checked separately from the development baseline above:

| Check | Result | Scope |
|---|---|---|
| Public-source unit suite after the CI path fix | **159 passed, 2 ignored by default** | Includes the added Windows short-path regression. The four earlier CI test failures were fixed before the successful run below. |
| Rebuilt local public-source runtime after that path fix | **68 worker cases passed** | 25 static-image/archive, 24 animation, and 19 real-camera/high-depth/metadata cases. The real photographs remain outside the repository. |
| Fresh GitHub Actions checkout after the fix | **Success** | `windows-2022`: engine preparation, 159 unit tests passed with 2 ignored, bootstrap build, 25 static-image/archive cases, and 24 animation cases. |

The completed [GitHub Actions run 42846831762](https://github.com/walker1911/RustView/actions/runs/42846831762) independently exercised **49 worker cases** from a fresh checkout. The optional 19-case compatibility suite with real-camera samples belongs to the local 68-case baseline, not to this CI run. These checks do not certify every GUI interaction or camera model, and subsystem budgets are not a total application memory ceiling.

## Complete offline package

The offline ZIP was extracted into a separate writable directory and tested using only the Windows system directories in `PATH`, with HTTP/HTTPS proxy variables pointing to an unavailable localhost endpoint. **25 static/archive, 24 GIF/WebP animation and 19 RAW/high-depth/metadata cases passed** from that extraction. No dependency setup script was included or invoked. These are local-file/worker checks, not a claim that a system firewall was changed or that all GUI interaction was validated.

The application binary is byte-identical to the previously verified v0.3.0 executable (SHA-256 `9c407f6131780c64e81f9e9e50f9c9d93da020c17bc108750982de23ee2ec684`). Native core hashes match the pinned manifest; a ZIP CRC check and all packaged per-file SHA-256 values passed. Import checks found all non-system DLL dependencies in the package. The launcher directly starts the executable and contains no downloader.

离线包从独立解压目录通过 25 项静态／压缩包、24 项动画、19 项 RAW／高位／元数据测试；只保留系统 PATH，未运行依赖准备脚本。应用二进制与先前版本相同，原生核心摘要、归档完整性及逐文件摘要均核验。真实 GUI 交互仍受前述验证范围限制。

## Memory observations

A 512×512 animation streamed for 1,200 frames without linear growth after warm-up. CLI process-tree private committed memory was approximately 10 MiB; that excludes GUI/font/driver/texture overhead. Real RAW PNG16 exports peaked around **472 MiB** in the sampled worker process tree. A 30MP Canon exposure plus white-balance export also passed within unchanged resource budgets. These are sample-specific observations, not maximum RSS or performance guarantees for arbitrary files.

## What is not verified

The Windows capture/automation interface timed out or denied access in the development environment. Headless UI logic and worker tests do not replace real-window, multi-monitor, DPI and visual testing. Wallpaper changes, real paper output, every camera/firmware and multi-hour use of a user's full library are not certified. No personal photographs, runtime databases, local diagnostic paths, reference binaries or private resource dumps are included in the public source or bootstrap package.

## Reproduce on Windows x64

Prepare the engines as described in [BUILDING.md](../BUILDING.md), then run:

```powershell
cargo test --locked -- --test-threads=2
.\tools\build.ps1 -OutputRoot .\test-output\local-package
python tools/smoke_test.py --exe test-output/local-package/RustView-0.3.0/RustView.exe --output test-output/smoke.json
python tools/animation_smoke.py --exe test-output/local-package/RustView-0.3.0/RustView.exe --output test-output/animation.json
```

The real-camera suite is optional and expects separate local sample files; it is not a default checkout requirement. Its script, `tools/compat_smoke.py`, documents the expected filenames and validates output depth/ICC, source digests and process-tree sampling. Sample sources include [the public RAW fixture release](https://github.com/tellodaniel/revelraw-sample-raw-files/releases/tag/v1), [libheif's test sample](https://github.com/strukturag/libheif), and [raw.pixls.us](https://raw.pixls.us/). Do not upload private images or credentials with an issue; use a minimal redistributable reproduction when possible.

## 中文摘要

开发版曾通过 158 项单测（2 项真实相机样本默认跳过）及 68 项独立进程回归；真实样本覆盖上述尼康、索尼、佳能、苹果 ProRAW 与 HEIC。图库缓存、解码进程、动画预读及打印文字有单独预算，示例内存观测不代表整个 GUI 的峰值。真实窗口交互、全部机型、长时间用户图库和实际纸张打印尚未全面验收。公开仓库保留合成及带许可的小夹具，真实照片和本地运行数据均不发布。

公开源码在修复 CI 路径处理问题并新增 Windows 短路径回归后，单测为 **159 通过、2 默认忽略**，重新编译的本地公开源码版本也已通过 **68 项工作进程回归**；修复后的 [GitHub Actions 干净检出](https://github.com/walker1911/RustView/actions/runs/42846831762)在 `windows-2022` 上通过单测、构建和 **49 项静态／动画回归**。真实相机／高位／元数据等 19 项兼容回归仅在本地运行，不计入 CI 的 49 项。
