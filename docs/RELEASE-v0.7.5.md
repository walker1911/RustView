# RustView v0.7.5 · 静态 SVG 与缩放补清晰 / Static SVG and sharper zoom

## 简体中文

- **静态 SVG 跟普通图片一样看。** 可从本地打开或拖入窗口，在底部预览缩略图，也能直接查看 ZIP、RAR、7z 内的 SVG。遇到加密压缩包时输入密码即可继续。要用 RustView 双击打开 `.svg`，可在 Windows 的「打开方式」中选定 RustView。
- **缩放时保留画面，停稳后补细节。** resvg 先渲染并显示概览，缩放操作停稳约 140 ms 后，后台按目标大小重新渲染。清晰图就绪后替换当前画面，透明区域的棋盘格固定在屏幕上。
- **支持范围。** 打开 UTF-8 编码的静态 `.svg`，源文件上限为 8 MiB，重新渲染时最长边为 4,096 像素且总面积最多 800 万像素。支持内嵌 PNG、JPEG、WebP，每张内嵌图限 4,194,304 像素。外链资源会跳过，SVGZ、脚本和动画暂不支持。仅在图中含文字时加载一组常用 Windows 字体，缺少原字体时字形和排版可能有差异。

### 与 v0.7.4 的本机实测

同一台 Windows 电脑交替启动两版，以桌面实际显示的像素确认首图。每次使用与图片关联相同的启动参数并创建新进程。OneDrive 桌面和本地目录使用字节相同、已完整下载到本机的 JPEG、PNG，每种格式和位置各计时 8 对。万张 JPEG 和混合 RAW 各计时 24 对，两轮调换起始版本。RAW 样本包含尼康 NEF、索尼 ARW、佳能 CR3 和 iPhone DNG。下表为预热后的数据。

| 首图 | v0.7.4 中位数 | v0.7.5 中位数 | 配对差值中位数 |
|---|---:|---:|---:|
| OneDrive 桌面 JPEG | 492 ms | 500 ms | +9.0 ms |
| OneDrive 桌面 PNG | 586 ms | 498 ms | −73.3 ms |
| 本地目录 JPEG | 471 ms | 505 ms | −1.9 ms |
| 本地目录 PNG | 502 ms | 513 ms | −5.8 ms |
| 万张 JPEG，首张清晰图 | 531 ms | 539 ms | +3.1 ms |
| 混合 RAW，首张清晰图 | 525 ms | 519 ms | +6.9 ms |

配对差值是每对先用新版时间减旧版时间，再取中位数。以本地 JPEG 为例，两版各自的中位数相差约 35 ms，逐对比较的中位数为 −1.9 ms。本地 PNG 的 P95 从 556 ms 升到 718 ms。这组单机预热数据的首图有快有慢，OneDrive 未下载的占位文件和系统冷启动另有耗时。

| 连续翻图与内存 | v0.7.4 | v0.7.5 |
|---|---:|---:|
| 万张 JPEG，长按到目标图出现（中位数） | 19.8 ms | 19.1 ms |
| 万张 JPEG，松键到清晰图（中位数） | 19.6 ms | 17.9 ms |
| 混合 RAW，长按到目标图出现（中位数） | 11.6 ms | 11.4 ms |
| 混合 RAW，松键到清晰图（中位数） | 289.8 ms | 281.3 ms |
| 万张 JPEG，进程树采样峰值 | 426.6 MiB | 429.0 MiB |
| 混合 RAW，进程树采样峰值 | 510.2 MiB | 512.5 MiB |

浏览测试约每 90 ms 采样 RustView 和解码子进程的私有提交内存，GPU 内存和系统文件缓存另计。两版连续翻图均未采到空白帧。新版另测了本地 SVG、拖入、加密 ZIP／RAR／7z 和超大画布缩放，确认透明棋盘不随缩放移动，取消读取和关窗后无残留进程或临时文件。289 项单元测试通过，3 项跳过。

### 完整离线版

完整解压 Windows x64 ZIP，再双击 `RustView.exe` 或 `Start-RustView.cmd`。包内含 7-Zip、LibRaw、ImageMagick、ExifTool、VC++ 运行库，以及 Tesseract 和简体中文、英文 OCR 模型。首次启动可离线使用。升级到新目录时，关闭 RustView 后将旧程序旁的 `RustView-data` 复制到新程序旁即可保留数据。[离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.5/RustView-0.7.5-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.7.5/RustView-0.7.5-win-x64-offline.zip.sha256)

## English

- **View static SVGs alongside other images.** Open a local file, drop it into the window, browse its thumbnail, or view it inside a ZIP, RAR, or 7z archive. Enter the password when prompted for an encrypted archive. To open `.svg` files by double-clicking, select RustView through Windows **Open with**.
- **Keep the image visible while zooming and restore detail when you pause.** resvg first renders an overview for display. When zoom input settles for about 140 ms, RustView renders the new size in the background and swaps in the sharper image when it is ready. The checkerboard stays fixed on screen.
- **Supported SVGs.** The viewer accepts UTF-8 static `.svg` files up to 8 MiB. Rerenders have a maximum edge of 4,096 pixels and a maximum area of 8 million pixels. Embedded PNG, JPEG, and WebP images are limited to 4,194,304 pixels each. External resources are skipped. SVGZ, scripts, and animation are outside the supported scope. A set of common Windows fonts loads only when the SVG contains text. Missing fonts may affect glyphs and layout.

### Measured against v0.7.4

Both versions ran in alternating pairs on the same Windows PC. Captured desktop pixels confirmed the first image. Each launch used a fresh process and the same arguments as a Windows file association. The OneDrive desktop and local folder contained identical, locally available JPEG or PNG bytes, with eight timed pairs per format and location. The 10,000-JPEG and mixed RAW workloads each had 24 timed pairs over two rounds with the starting version reversed. The RAW set included Nikon NEF, Sony ARW, Canon CR3, and iPhone DNG. These are warm-run results.

| Time to first image | v0.7.4 median | v0.7.5 median | Median paired difference |
|---|---:|---:|---:|
| OneDrive desktop JPEG | 492 ms | 500 ms | +9.0 ms |
| OneDrive desktop PNG | 586 ms | 498 ms | −73.3 ms |
| Local-folder JPEG | 471 ms | 505 ms | −1.9 ms |
| Local-folder PNG | 502 ms | 513 ms | −5.8 ms |
| 10,000 JPEGs, first clear image | 531 ms | 539 ms | +3.1 ms |
| Mixed RAW, first clear image | 525 ms | 519 ms | +6.9 ms |

The paired difference subtracts the v0.7.4 time from the v0.7.5 time within each pair, then takes the median. For local JPEG, the individual medians are about 35 ms apart and the median paired difference is −1.9 ms. Local PNG P95 increased from 556 ms to 718 ms. First-image times varied across this single-PC warm-run test. Opening an online-only OneDrive file or launching with cold caches can add time.

| Continuous browsing and memory | v0.7.4 | v0.7.5 |
|---|---:|---:|
| 10,000 JPEGs, held arrow to visible target (median) | 19.8 ms | 19.1 ms |
| 10,000 JPEGs, key release to clear target (median) | 19.6 ms | 17.9 ms |
| Mixed RAW, held arrow to visible target (median) | 11.6 ms | 11.4 ms |
| Mixed RAW, key release to clear target (median) | 289.8 ms | 281.3 ms |
| 10,000 JPEGs, highest sampled process-tree commit | 426.6 MiB | 429.0 MiB |
| Mixed RAW, highest sampled process-tree commit | 510.2 MiB | 512.5 MiB |

The browsing tests sampled private committed memory in RustView and its decoder processes about every 90 ms. GPU memory and the system file cache are outside this measurement. Neither version showed a blank frame in the browsing samples. Additional v0.7.5 checks covered local SVGs, file drops, encrypted ZIP, RAR and 7z, and zooming large SVG canvases. They confirmed the fixed checkerboard and found no leftover worker processes or temporary files after cancellation or closing the window. 289 unit tests passed and three were skipped.

### Complete offline version

Extract the entire Windows x64 ZIP, then double-click `RustView.exe` or `Start-RustView.cmd`. The bundle includes 7-Zip, LibRaw, ImageMagick, ExifTool, the app-local VC++ runtime, and Tesseract with Simplified Chinese and English OCR models. It works offline on first launch. For an upgrade to a new folder, close RustView and copy the old `RustView-data` directory alongside the new executable to keep your data. [Offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.5/RustView-0.7.5-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.7.5/RustView-0.7.5-win-x64-offline.zip.sha256)
