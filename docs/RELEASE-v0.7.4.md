# RustView v0.7.4 · 文件夹续看与首图优化 / Folder resume and first-image performance

## 简体中文

- **在「文件 → 最近打开」继续看本地文件夹。** 点击文件夹回到上次显示的图片，点「从头打开」则直接看第一张。浏览时改名会跟随新文件名。关窗后改名或删除导致旧文件名找不到时，续看会回到第一张。目标图还没显示就关窗时，保留上次的续看位置。双击具体图片仍从该图打开。
- **缩短解码完成到出图的等待。** 直接打开图片时，已就绪的首图会请求窗口立即刷新。无需缩小的概览复用已解码的像素，减少临时文件读写。解码进程退出时立即收取结果，继续保留取消读取和内存控制。

### 与 v0.7.3 的本机实测

两版在同一台 Windows 电脑上交替运行，用桌面实际显示的像素判断首图和清晰度。OneDrive 桌面与本地测试目录使用字节相同且已写入本机的 JPEG／PNG，每种格式、每个位置各测 6 对。每次打开都启动独立进程，使用与图片关联相同的参数。万张 JPEG 测了 8 对加 12 对，混合 RAW 测了 8 对，包含尼康 NEF、佳能 CR3、索尼 ARW 和 iPhone DNG。以下为预热后的中位数。

| 首张清晰图 | v0.7.3 | v0.7.4 |
|---|---:|---:|
| OneDrive 桌面 JPEG | 536 ms | 521 ms |
| OneDrive 桌面 PNG | 556 ms | 503 ms |
| 本地目录 JPEG | 508 ms | 508 ms |
| 本地目录 PNG | 541 ms | 529 ms |
| 万张 JPEG，合计 20 对 | 569 ms | 578 ms |
| 混合 RAW | 554 ms | 536 ms |

**万张 JPEG 的首图没有稳定提速。** 两轮分别为 567 → 591 ms 和 574 → 565 ms，合计中位数慢约 9 ms。分段追踪中，JPEG 样本的解码结果早于窗口显示就绪，显卡初始化和首帧显示仍占较多时间。

| 连续翻图与内存 | v0.7.3 | v0.7.4 |
|---|---:|---:|
| 万张 JPEG，长按到目标图出现（中位数） | 20 ms | 20 ms |
| 万张 JPEG，点按回看缓存到清晰图（中位数） | 33 ms | 33 ms |
| 混合 RAW，长按到目标图出现（中位数） | 12 ms | 15 ms |
| 混合 RAW，松键到清晰图（中位数） | 311 ms | 299 ms |
| 万张 JPEG，进程树采样最高值 | 427.3 MiB | 428.9 MiB |
| 混合 RAW，进程树采样最高值 | 509.7 MiB | 509.2 MiB |

私有提交内存约每 90 毫秒采样一次，统计 RustView 和解码子进程，GPU 内存和系统文件缓存另计。连续翻图未采到空白帧，测试关窗后未发现残留进程或临时文件。续看、从头打开、改名、删除和未出图关窗场景均通过实测。280 项单元测试通过，3 项跳过。

### 完整离线版

完整解压 Windows x64 ZIP，再双击 `RustView.exe` 或 `Start-RustView.cmd`。随包提供 7-Zip、LibRaw、ImageMagick、ExifTool、VC++ 运行库，以及 Tesseract 和简体中文、英文 OCR 模型，首次启动即可离线看图、扫码和识字。升级到新目录时，先关闭 RustView，再把旧程序旁的 `RustView-data` 复制到新程序旁，以保留原有数据。[离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.4/RustView-0.7.4-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.7.4/RustView-0.7.4-win-x64-offline.zip.sha256)

## English

- **Resume a local folder from File → Recent Opens.** Select a folder to return to the last picture you saw, or choose **Open from Start** to see its first picture. A rename while viewing follows the new filename. If the saved filename is gone after a rename or deletion while RustView is closed, the folder starts at its first picture. Closing before the target appears preserves the previous position. Opening a specific picture still starts with that file.
- **Shorter waits between decoding and display.** When directly opening an image, RustView requests a redraw as soon as the initial image is ready. If the overview needs no scaling, it reuses the decoded pixels and skips duplicate temporary-file I/O. The viewer collects results as soon as the decoder exits while retaining cancellation and memory controls.

### Measured against v0.7.3

Both versions ran in alternating pairs on the same Windows PC. Desktop captures confirmed the target pixels and visible detail. Each format and location had six pairs using identical locally available JPEG or PNG bytes on the OneDrive desktop and in a local folder. Each launch used a fresh process with the same arguments as a Windows file association. The 10,000-JPEG workload had two rounds of 8 and 12 pairs. The mixed RAW workload had eight pairs with Nikon NEF, Canon CR3, Sony ARW, and iPhone DNG. The table shows warm-run medians.

| Time to first clear image | v0.7.3 | v0.7.4 |
|---|---:|---:|
| OneDrive desktop JPEG | 536 ms | 521 ms |
| OneDrive desktop PNG | 556 ms | 503 ms |
| Local-folder JPEG | 508 ms | 508 ms |
| Local-folder PNG | 541 ms | 529 ms |
| 10,000 JPEGs, 20 pairs combined | 569 ms | 578 ms |
| Mixed RAW | 554 ms | 536 ms |

**The 10,000-JPEG workload showed no consistent first-image speedup.** The rounds measured 567 → 591 ms and 574 → 565 ms. The combined median was about 9 ms slower. Startup traces showed the JPEG decode result ready before the window appeared, with graphics initialization and first-frame presentation still taking much of the time.

| Continuous browsing and memory | v0.7.3 | v0.7.4 |
|---|---:|---:|
| 10,000 JPEGs, held arrow to visible target (median) | 20 ms | 20 ms |
| 10,000 JPEGs, cached return tap to clear image (median) | 33 ms | 33 ms |
| Mixed RAW, held arrow to visible target (median) | 12 ms | 15 ms |
| Mixed RAW, key release to clear image (median) | 311 ms | 299 ms |
| 10,000 JPEGs, highest sampled process-tree commit | 427.3 MiB | 428.9 MiB |
| Mixed RAW, highest sampled process-tree commit | 509.7 MiB | 509.2 MiB |

Private commit was sampled about every 90 ms across RustView and its decoder processes. GPU memory and the system file cache are outside that measurement. Continuous browsing sampled zero blank frames. The test windows closed with no remaining worker processes or temporary files. Desktop tests covered resume, starting over, rename, deletion, and closing before a target appeared. 280 unit tests passed and 3 were ignored.

### Complete offline package

Fully extract the Windows x64 ZIP and double-click `RustView.exe` or `Start-RustView.cmd`. The package includes 7-Zip, LibRaw, ImageMagick, ExifTool, app-local VC++ runtime libraries, and Tesseract with Simplified Chinese and English OCR models. Image viewing, QR detection, and OCR work offline from first launch. When upgrading to a new folder, close RustView and copy `RustView-data` from beside the old executable to the new folder to keep your existing data. [Offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.4/RustView-0.7.4-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.7.4/RustView-0.7.4-win-x64-offline.zip.sha256)

[离线版说明 / Offline guide](https://github.com/walker1911/RustView/blob/v0.7.4/OFFLINE.md) · [上个版本 / Previous release](https://github.com/walker1911/RustView/releases/tag/v0.7.3)
