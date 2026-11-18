# Validation / 验证记录

## v0.8.0 Windows x64

本地单测通过 **301 项**，默认跳过 3 项可选测试。原生窗口用同尺寸 PSD 和透明 PNG 验证了缩放、平移、1:1 像素对齐、按住看 A、松手回 B，以及快速左右切换 B。尼康 Z6、佳能 EOS R 和索尼 a7 III 的 RAW 样本也通过了 A/B 切换。两组关窗后均未发现残留进程或临时文件。

与 v0.7.9 在同一台电脑交替测试。万张 JPEG 和混合 RAW 各 24 对，241 张图的 ZIP 测 12 对。混合 RAW 首张清晰图的中位数为 513 → 534 ms，两轮均稍慢。 [v0.8.0 更新说明与实测数据](RELEASE-v0.8.0.md)

The local suite passed **301 unit tests**, with three optional tests skipped. Native Windows checks covered zoom, pan, 1:1 alignment, holding to show A, releasing to show B, and rapid B changes with a same-size PSD and transparent PNGs. Switching also passed with Nikon Z6, Canon EOS R, and Sony a7 III RAW samples. Both sessions closed with no surviving processes or temporary files.

The same-PC comparison against v0.7.9 used 24 pairs each for 10,000 JPEGs and mixed RAW and 12 pairs for a 241-image ZIP. Median time to the first sharp mixed RAW image went from 513 to 534 ms and was slightly slower in both batches. [v0.8.0 release notes and benchmarks](RELEASE-v0.8.0.md#english)

## v0.7.9 Windows x64

本地 Windows x64 单测通过 **301 项**，默认跳过 3 项可选测试。原生窗口测试用三个 241 页的压缩包验证了底部页码入口、拖动时的目标信息和缓存缩略图、松手后的目标读取，以及左右键继续翻图。ZIP、加密固实 RAR 和 7z 各自都在拖动期间保持零提取，松手后提取目标页一次。Esc、读取途中点击「取消」和关窗测试均通过，续看位置仍为最后实际显示的图片，观测到的工作进程已退出，临时目录为空。

在同一套离线运行库下交替测试 v0.7.8 和 v0.7.9。三个压缩包各 6 对，万张 JPEG 和混合 RAW 各 12 对，另有本地目录和 OneDrive 桌面上同一份在本地可用的 JPEG 的启动跟踪，各 8 对。同步置顶窗口给旧测试脚本带来约 190–289 毫秒的采样开销。修正采样后，仍观察到偶发 JPEG 首图慢样本。[v0.7.9 更新说明与实测数据](RELEASE-v0.7.9.md)

The local Windows x64 suite passed **301 unit tests**, with three optional tests skipped by default. Native-window tests used three 241-page archives to check the bottom counter, target information and cached thumbnails during dragging, the read on release, and continued arrow-key browsing. Dragging started zero extractions in the ZIP, encrypted solid RAR, and encrypted solid 7z. Each release extracted the selected page once. Tests for Esc, Cancel during a read, and closing mid-read kept the resume position on the last image actually shown. All observed workers exited and the temporary directories were empty.

Alternating runs used the same offline runtime for v0.7.8 and v0.7.9. There were six pairs per archive, 12 pairs each for 10,000 JPEGs and mixed RAW, and eight startup-trace pairs each from a local folder and the OneDrive Desktop with the JPEG available locally. A synchronous topmost-window call added about 190–289 ms in the earlier capture harness. Occasional slow JPEG startup samples remained after correcting capture. [v0.7.9 release notes and benchmarks](RELEASE-v0.7.9.md#english)

## v0.7.8 Windows x64

本地 Windows x64 单测通过 **299 项**，默认跳过 3 项可选测试。桌面实测覆盖文件夹、ZIP、加密 RAR 和加密 7z 的置顶、重启后续看、从头打开和取消置顶。ZIP 在预览中反向翻图、取消读取和关窗后，所观测的工作进程均已退出，临时目录为空。

用同一套离线依赖与 v0.7.7 交替运行。普通 ZIP 和预读转前台各 10 对，万张 JPEG 和混合 RAW 各 12 对，另外为 JPEG 首图做了 8 对启动跟踪。覆盖首图、长按预览、清晰图和进程树采样峰值。JPEG 首图仍有波动。详细 P50／P95、进程树和独立 RAW 解码结果见发布说明。[v0.7.8 更新说明](RELEASE-v0.7.8.md)

The local Windows x64 suite passed **299 tests**, with three optional tests skipped by default. Native-window tests covered pinning, resuming after restart, opening from start, and unpinning for a local folder, ZIP, encrypted RAR, and encrypted 7z. After reversing direction, cancelling a read, or closing the viewer during ZIP preview, all observed workers exited and the temporary directory was empty.

The same offline runtime was used for alternating runs against v0.7.7. There were 10 pairs each for ordinary ZIP and prefetched ZIP selection, 12 pairs each for 10,000 JPEGs and mixed RAW, and eight additional pairs for JPEG startup traces. The tests covered initial display, held-arrow previews, sharp images, and sampled process-tree peaks. JPEG startup times still varied. [v0.7.8 release notes and benchmark methods](RELEASE-v0.7.8.md#english)

## v0.6.0 Windows x64

The local Windows suite passed **220 unit tests**, with two optional public-camera tests ignored by default. The new tests cover archive jump bounds and ordering, ambiguous filenames, exact entry identity, and reopening from the local catalog.

| Check | Result | Scope |
|---|---:|---|
| Resume and jump in encrypted ZIP, RAR and 7z | 3 formats passed | The test drove the Viewer and egui dialogs with input events, used real supervised extraction workers, then reopened a fresh Viewer against the same isolated SQLite catalog. Each format passed both ordinal and filename jumps, prompted for its password again, and restored a 521 MiB PSD. |
| Encrypted PSD worker reads | 9 images passed | The v0.6.0 executable decoded all test entries in three encrypted archives, including the 521 MiB PSD in each archive and a 1.67 GiB PSD in the RAR. |
| Same-Viewer navigation | 8 passed | Verified image identity, preview continuity, cancellation, close and cleanup with real workers and headless drawing. |
| Large encrypted RAR5 | 76 passed | Covered data and header encryption, password handling, decode, thumbnails, export and repeated cancellation. All observed processes exited, test temporary directories were empty, and fixture hashes matched their original values. |

The resume test used encrypted ZIP and 7z archives of about 521 MiB each and a 2.18 GiB encrypted RAR. Closing during extraction of a different RAR member cancelled an observed partial extraction. Reopening restored the last displayed PSD. The catalog passed SQLite integrity checks and contained no test password. The supervised worker probes and the Viewer test both checked for surviving processes and temporary files.

The resume and jump results exercise the same Viewer and dialog logic in a headless test build. Desktop automation timed out while waiting for app access, leaving native v0.6.0 window clicks unverified.

### v0.6.0 中文验证

Windows x64 本地测试通过 **220 项单测**，另有 2 项公开相机测试默认跳过。加密 ZIP、RAR、7z 均通过按序号跳转、按文件名跳转和重开续看，重开时再次询问密码，并恢复到上次显示的 521 MiB PSD。ZIP 和 7z 约为 521 MiB，RAR 为 2.18 GiB。RAR 测试在提取另一张大图时中途关窗，重开仍回到上一张已显示的图片。SQLite 完整性检查通过，目录库中没有测试密码。

v0.6.0 可执行文件另外通过 9 张加密包内图片的读取验证、8 项同 Viewer 无窗口导航回归和 76 项大型加密 RAR 回归。已观测的工作进程均退出，测试临时目录为空。续看和跳页由同一套 Viewer 与 egui 对话框逻辑驱动，解压由真实的 Windows 工作进程完成。原生窗口自动化的应用访问审批超时，v0.6.0 的续看和跳页未完成原生窗口点击验证。

## v0.5.0 browsing validation / 浏览验证

| Native Windows session | Verified behavior | Sampled process-tree private commit peak |
|---|---|---:|
| 10,000 JPEG files | 12 rapid forward steps followed by 8 backward steps reached image 5,437. | 696 MiB |
| Real camera and phone RAW | Opened Nikon Z6, Sony a7 III, Canon EOS R and iPhone 12 Pro samples. The separate worker suite passed full decode for all four. | 798 MiB |
| Large encrypted RAR | Opened 521 MiB and 1.67 GiB PSDs after password entry. Reversing direction, the Cancel button and Alt+F4 interrupted extraction. | 424 MiB |

The encrypted RAR window stayed responsive after cancellation. An idle checkpoint observed no live workers or temporary files. After the final close, all 83 observed processes had exited and the test temporary directory was empty. Memory figures are observations for these inputs and include the app and its sampled descendants. GPU allocations and the OS file cache are outside these figures.

A preflight on the same startup path measured the time to deliver full target-image pixels from the worker in the 10,000-image directory. The P50 and P95 changed from 766 and 844 ms to 657 and 704 ms respectively. These timings use worker output. The native-window automation verified the final selected image, and frame capture timed out. Separate same-Viewer drawing checks verified that rapid navigation retained an image after the first display.

### v0.5.0 中文验证

原生窗口在 10,000 张 JPEG 的目录中向前连翻 12 次、向后连翻 8 次，到达正确的第 5,437 张。尼康 Z6、索尼 a7 III、佳能 EOS R 和 iPhone 12 Pro 的 RAW 样本均在窗口打开，独立工作进程也通过了四份样本的完整解码。大型加密 RAR 输密码后打开了 521 MiB 和 1.67 GiB 的 PSD，反向翻图、点取消和 Alt+F4 均能中止正在进行的提取。点取消后窗口可继续使用，关窗后已观测的 83 个进程全部退出，临时目录为空。

这三组原生窗口的进程树私有提交内存峰值分别为 696、798 和 424 MiB。相同启动路径的预检中，万张目录目标图完整像素交付的 P50 从 766 ms 降至 657 ms，P95 从 844 ms 降至 704 ms。计时取自工作进程输出。原生窗口逐帧抓取超时，首图后的连续画面通过同一 Viewer 的绘制回归核对。

## v0.4.1 prerelease

The Windows x64 build passed **209 unit tests with two public-camera tests ignored by default**. The rendering regression verifies the gray and white checkerboard palette, its fixed window coordinates during zoom and rotation, clipping to image bounds, and preservation of full and partial alpha. The transition-preview check verifies that the checkerboard sits beneath a single current preview.

A fresh offline ZIP extraction passed **87 checks**. The test environment used a system-only PATH and unavailable localhost HTTP, HTTPS and ALL proxies. The launcher directly starts RustView from the extracted folder.

| Check | Passed |
|---|---:|
| Static images and archives | 25 |
| GIF and WebP animation | 24 |
| Thumbnails | 9 |
| PSD and PSB | 21 |
| Same-Viewer headless navigation | 8 |

The package passed ZIP integrity and all per-file SHA-256 checks. Native engines, their support files and app-local runtime DLLs matched the verified v0.4.0 runtime by SHA-256. The embedded manifest identifies the executable as 0.4.1.0. Its SHA-256 is `9a3f86ea5e503a1fc9efa93b6d6b987e3ab3b3f13193a439d54cfa8e61ad7d7b`. GitHub Actions results are linked on the release page.

### v0.4.1 中文验证

Windows x64 版通过 **209 项单测，2 项公开相机测试默认跳过**。棋盘格回归核对了灰白底色、放大与旋转时的窗口坐标、裁剪边界以及全透明和半透明像素。独立解压目录通过 **87 项检查**，包括 25 项静态图与压缩包、24 项动画、9 项缩略图、21 项 PSD／PSB 和 8 项无窗口连续翻图。逐文件校验和 ZIP 完整性检查通过，原生依赖与 v0.4.0 的已验证运行包逐项匹配。远端构建记录见 Release。

## v0.4.0 prerelease — versioned rebuild checks

The versioned Windows x64 executable passed **208 unit tests (2 public-camera tests ignored by default)** and **217 package-level checks** from an independent offline-ZIP extraction: 209 isolated-process cases plus eight same-Viewer headless navigation cases. These are the final rebuilt artifact's results, not copied counts from a development executable. The release remains a **prerelease**.

| Check | Result | Scope |
|---|---|---|
| Windows x64 binary | Passed | Embedded manifest 0.4.0.0; EXE SHA-256 `7889a6571a3db3b91462551cc92ef1d6f065f8a204f641dba0883cf26931b662`. |
| Static images / archives | 25 passed | Common formats, large pixels, passwords, errors and archive boundaries. |
| GIF / WebP animation | 24 passed | Frame composition, timing, loops, cancellation and streamed frames. |
| RAW / high-depth / metadata | 19 passed | Nikon Z6, Sony a7 III, Canon EOS R, iPhone 12 Pro DNG and libheif HEIC samples; real PNG16/ICC and metadata checks. Samples are not distributed. |
| Thumbnails | 9 passed | Small bounded results, archive/animation/RAW paths and source geometry. |
| PSD / PSB | 21 passed | Local/archive composites, password, thumbnails, single-page and single-frame export behavior. |
| Large ZIP / RAR / 7z PSD | 35 passed | Sources above 512 MiB, AES ZIP/7z, partial-extraction cancellation and temporary cleanup. |
| Large encrypted RAR5 | 76 passed | Data and header encryption, missing/wrong/correct passwords, physical containers above 512 MiB, pixels, exports, 16 actual cancels and observed process termination. |
| Same-Viewer navigation | 8 passed | 10/250 ms navigation, pixel identity, no welcome frames after a seed image, stale-result isolation, close/drain and cleanup. Headless; not visual-window certification. |
| Windows registration | Passed | One isolated-registry regression script checks 54 image registrations and three archive verbs, ownership, repeat install/remove and preserved defaults. These are not 54 image-decoding cases. |
| Offline dependencies | Passed | System-only PATH and unavailable localhost HTTP/HTTPS/ALL proxies; no dependency downloader included or invoked. The source writer is test-only. |
| Package integrity | Passed | ZIP CRC and all shipping SHA-256 values checked; x64 core runtime and app-local VC++ libraries present. Final ZIP checksum accompanies the download. |
| GitHub Actions | Reported separately | See the release's linked Windows x64 run and [Actions](https://github.com/walker1911/RustView/actions/workflows/ci.yml). CI uses public fixtures; local real-camera and large encrypted-RAR counts are not CI counts. |

The release check exposed a cancellation race leaving codec scratch outside the supervised task. Worker TEMP/TMP now point into the request directory so the supervisor cleans that scratch even when the worker is forcibly stopped. The process monitor also verifies creation times against the held parent's lifetime: an old background process with a recycled numeric parent PID must not be counted as a new RustView descendant. Memory sampling uses the same verified process handles; thresholds were not relaxed.

The 76 RAR cases require every observed owned process to exit and every request TEMP to become empty. Early cancellation observes real incomplete PSD extraction. Sampling can miss very short-lived descendants. Private committed memory excludes Python and reclaimable OS file cache; the small synthetic composites do not establish a GUI total-memory ceiling or arbitrary-PSD performance guarantee.

Native-window password retry, navigation and Alt+F4 during a physical large encrypted RAR read were verified on the preceding implementation with UI automation. That is separate from this rebuilt artifact's headless checks. Screenshot capture, full visual/multi-DPI/multi-monitor acceptance, paper printing and multi-hour user-library sessions remain outside the completed evidence.

Matching native sources and hashes are in [NATIVE-SOURCES.md](NATIVE-SOURCES.md). Historical v0.3 results below apply only to that release.

### Reproduce the v0.4.0 build

```powershell
cargo test --locked -- --test-threads=2
.\tools\build.ps1 -Offline -OutputRoot .\test-output\v040-package
python tools/smoke_test.py --exe test-output/v040-package/RustView-0.4.0/RustView.exe --output test-output/v040-smoke.json
python tools/animation_smoke.py --exe test-output/v040-package/RustView-0.4.0/RustView.exe --output test-output/v040-animation.json
```

Prepare pinned native engines as described in [BUILDING.md](../BUILDING.md) before building. The extra PSD/RAR/navigation suites and optional real-camera suite must name the same rebuilt executable when results are added. Headless state/worker probes are not real-window, DPI, multi-monitor, paper-printing or multi-hour-library certification.

### v0.4.0 中文说明

版本化 v0.4.0 已通过208项单测（2项公开相机测试默认跳过），独立离线解压目录通过217项检查：209项工作进程回归及8项同Viewer无窗口导航回归。另一个隔离注册表脚本核验54种图片候选注册与3种压缩包右键入口，不能把它写成54项解码测试。修复了探测阶段取消后解码临时目录遗漏，并排除了进程PID复用造成的监测误报；最终RAR测试要求观测到的产品进程退出、每次临时目录为空。离线验证只保留系统PATH并指向不可用代理，未调用下载脚本；没有改系统防火墙。EXE摘要见上表，最终ZIP摘要见下载旁的.sha256。CI结果单独在Release链接；真实相机和大型RAR本地测试不冒充CI结果。预发布、截图、多屏DPI、纸张打印及长时间用户图库的验证边界保持明确。

## v0.3 historical validation / 历史记录

The remainder of this document preserves the completed v0.3 evidence and reproduction commands. Check out the corresponding v0.3 tag before using paths containing `RustView-0.3.0`; they are not the current release paths.

### v0.3 development baseline

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

### Public-source release checks

The public source was checked separately from the development baseline above:

| Check | Result | Scope |
|---|---|---|
| Public-source unit suite after the CI path fix | **159 passed, 2 ignored by default** | Includes the added Windows short-path regression. The four earlier CI test failures were fixed before the successful run below. |
| Rebuilt local public-source runtime after that path fix | **68 worker cases passed** | 25 static-image/archive, 24 animation, and 19 real-camera/high-depth/metadata cases. The real photographs remain outside the repository. |
| Fresh GitHub Actions checkout after the fix | **Success** | `windows-2022`: engine preparation, 159 unit tests passed with 2 ignored, bootstrap build, 25 static-image/archive cases, and 24 animation cases. |

The completed [GitHub Actions run 42846831762](https://github.com/walker1911/RustView/actions/runs/42846831762) independently exercised **49 worker cases** from a fresh checkout. The optional 19-case compatibility suite with real-camera samples belongs to the local 68-case baseline, not to this CI run. These checks do not certify every GUI interaction or camera model, and subsystem budgets are not a total application memory ceiling.

### Complete offline package

The offline ZIP was extracted into a separate writable directory and tested using only the Windows system directories in `PATH`, with HTTP/HTTPS proxy variables pointing to an unavailable localhost endpoint. **25 static/archive, 24 GIF/WebP animation and 19 RAW/high-depth/metadata cases passed** from that extraction. No dependency setup script was included or invoked. These are local-file/worker checks, not a claim that a system firewall was changed or that all GUI interaction was validated.

The application binary is byte-identical to the previously verified v0.3.0 executable (SHA-256 `9c407f6131780c64e81f9e9e50f9c9d93da020c17bc108750982de23ee2ec684`). Native core hashes match the pinned manifest; a ZIP CRC check and all packaged per-file SHA-256 values passed. Import checks found all non-system DLL dependencies in the package. The launcher directly starts the executable and contains no downloader.

离线包从独立解压目录通过 25 项静态／压缩包、24 项动画、19 项 RAW／高位／元数据测试；只保留系统 PATH，未运行依赖准备脚本。应用二进制与先前版本相同，原生核心摘要、归档完整性及逐文件摘要均核验。真实 GUI 交互仍受前述验证范围限制。

### Memory observations

A 512×512 animation streamed for 1,200 frames without linear growth after warm-up. CLI process-tree private committed memory was approximately 10 MiB; that excludes GUI/font/driver/texture overhead. Real RAW PNG16 exports peaked around **472 MiB** in the sampled worker process tree. A 30MP Canon exposure plus white-balance export also passed within unchanged resource budgets. These are sample-specific observations, not maximum RSS or performance guarantees for arbitrary files.

### What is not verified

The Windows capture/automation interface timed out or denied access in the development environment. Headless UI logic and worker tests do not replace real-window, multi-monitor, DPI and visual testing. Wallpaper changes, real paper output, every camera/firmware and multi-hour use of a user's full library are not certified. No personal photographs, runtime databases, local diagnostic paths, reference binaries or private resource dumps are included in the public source or bootstrap package.

### Reproduce on Windows x64

Prepare the engines as described in [BUILDING.md](../BUILDING.md), then run:

```powershell
cargo test --locked -- --test-threads=2
.\tools\build.ps1 -OutputRoot .\test-output\local-package
python tools/smoke_test.py --exe test-output/local-package/RustView-0.3.0/RustView.exe --output test-output/smoke.json
python tools/animation_smoke.py --exe test-output/local-package/RustView-0.3.0/RustView.exe --output test-output/animation.json
```

The real-camera suite is optional and expects separate local sample files; it is not a default checkout requirement. Its script, `tools/compat_smoke.py`, documents the expected filenames and validates output depth/ICC, source digests and process-tree sampling. Sample sources include [the public RAW fixture release](https://github.com/tellodaniel/revelraw-sample-raw-files/releases/tag/v1), [libheif's test sample](https://github.com/strukturag/libheif), and [raw.pixls.us](https://raw.pixls.us/). Do not upload private images or credentials with an issue; use a minimal redistributable reproduction when possible.

### 中文摘要

开发版曾通过 158 项单测（2 项真实相机样本默认跳过）及 68 项独立进程回归；真实样本覆盖上述尼康、索尼、佳能、苹果 ProRAW 与 HEIC。图库缓存、解码进程、动画预读及打印文字有单独预算，示例内存观测不代表整个 GUI 的峰值。真实窗口交互、全部机型、长时间用户图库和实际纸张打印尚未全面验收。公开仓库保留合成及带许可的小夹具，真实照片和本地运行数据均不发布。

公开源码在修复 CI 路径处理问题并新增 Windows 短路径回归后，单测为 **159 通过、2 默认忽略**，重新编译的本地公开源码版本也已通过 **68 项工作进程回归**；修复后的 [GitHub Actions 干净检出](https://github.com/walker1911/RustView/actions/runs/42846831762)在 `windows-2022` 上通过单测、构建和 **49 项静态／动画回归**。真实相机／高位／元数据等 19 项兼容回归仅在本地运行，不计入 CI 的 49 项。
