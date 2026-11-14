# RustView v0.7.0 · 本地目录自动刷新 / Live local folder refresh

## 中文

- **自动跟进当前目录。** 添加、移走、重命名或保存图片后，文件列表和缩略图会自动更新。当前图被重命名时会跟随它，移走时会切到仍可查看的相邻图片。改过的图片会重新读取，未改动的图片可继续使用缓存。自动刷新适用于正在浏览的 Windows 本地目录。
- **PSD 保存后继续看同一处。** 当前图被 Photoshop 等软件原地保存或用临时文件替换时，RustView 会保留画面，等新内容能读取再原位更新。缩放和平移跟随原视图。连续按键翻图或前台正在读取时，目录更新会等浏览停下来。

### Windows 实测

同一份 PSD 在缩放和平移后，原子替换约 **315 毫秒**显示新内容，分段原地写入完成后约 **295 毫秒**。两次更新前后的画面区域一致，原子替换测试特意保留了原文件的大小和修改时间。分段写入中采样的 20 帧均保留上一版画面。另验证了新增、重命名（包括仅修改大小写）、删除、已缓存邻图被替换及待刷新时关闭窗口。

下表对照 v0.6.9 和 v0.7.0 的耗时中位数。桌面测试在同一台 Windows 电脑交替运行，两版共用已预热的系统文件缓存。JPEG 和 RAW 各测 8 对，排除预热。AES ZIP 从独立解码进程计时。加密压缩包内的 PSD 为 521 MiB。

| 测试项目 | v0.6.9 | v0.7.0 |
|---|---:|---:|
| 万张 JPEG 图库启动到首图 | 698 ms | 684 ms |
| 万张 JPEG 长按翻图到目标预览 | 79 ms | 79 ms |
| 相机 RAW 启动到首图 | 690 ms | 688 ms |
| RAW 长按翻图到目标预览 | 153 ms | 153 ms |
| RAW 松键到清晰图 | 269 ms | 273 ms |
| 加密 RAR 内大 PSD 显示 | 687 ms | 682 ms |
| 加密 7z 内大 PSD 显示，含预读 | 54 ms | 58 ms |
| AES ZIP 内大 PSD 解码 | 937 ms | 922 ms |
| 1,120 次目录变更期间，长按翻图到目标预览 | 124 ms | 98 ms |

常规 JPEG 和 RAW 的长按预览中位数接近，P95 各增加约 17 毫秒。目录变更压力测试中，目标预览的 P95 为 **169 → 170 毫秒**。采样所得的进程树私有提交内存最高值为 **587 → 588 MiB**（万张 JPEG）、**603 → 603 MiB**（RAW）和 **585 → 586 MiB**（目录变更压测）。采样不含 GPU 和系统文件缓存。桌面测试未采到空白帧，结束后未发现残留工作进程或任务临时文件。通过 272 项单元测试，3 项跳过。

下载 Windows x64 完整离线 ZIP，解压后双击 `RustView.exe` 或 `Start-RustView.cmd`。随包提供 7-Zip、LibRaw、ImageMagick、ExifTool、VC++ 运行库和 Tesseract 简体中文及英文模型。首次启动即可离线看图和识字。换目录时，关闭 RustView 后复制旧程序旁的 `RustView-data`，即可带上设置、快捷键、目录库和处理记录。[离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.0/RustView-0.7.0-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.7.0/RustView-0.7.0-win-x64-offline.zip.sha256)

## English

- **Follow changes to the active folder.** The file list and thumbnails update automatically when images are added, removed, renamed, or saved. RustView follows the current image after a rename and moves to a surviving neighbor if it is removed. Changed images are read again, and unchanged images can stay cached. Automatic refresh applies to the active local folder on Windows.
- **Keep your place during a PSD save.** When Photoshop or another editor writes over the current file or replaces it with a temporary file, RustView keeps the current picture visible until the new content can be read. The update preserves zoom and pan. Folder changes wait for a pause during held-key navigation or foreground loading.

### Windows measurements

With the same PSD zoomed and panned, the replacement appeared in about **315 ms** after an atomic save and **295 ms** after an in-place write finished. The sampled image footprint matched before and after both updates. The atomic replacement preserved the original size and modification time. All 20 frames sampled during the partial write showed the previous image. Desktop checks also covered addition, rename (including case-only changes), removal, replacement of a cached neighbor, and closing with refresh pending.

The table compares median times for v0.6.9 and v0.7.0. Desktop runs alternated versions on one Windows PC using a shared warm filesystem cache. The JPEG and RAW workloads each had eight measured pairs after excluding warm-ups. AES ZIP timing came from the isolated decode worker. The PSD inside each encrypted archive was 521 MiB.

| Workload | v0.6.9 | v0.7.0 |
|---|---:|---:|
| Launch to first image in a 10,000-JPEG folder | 698 ms | 684 ms |
| Held-arrow navigation to target preview in the JPEG folder | 79 ms | 79 ms |
| Launch to first image with camera RAW files | 690 ms | 688 ms |
| Held-arrow RAW navigation to target preview | 153 ms | 153 ms |
| RAW key release to clear image | 269 ms | 273 ms |
| Display large PSD in encrypted RAR | 687 ms | 682 ms |
| Display large PSD in encrypted 7z, including prefetch | 54 ms | 58 ms |
| Decode large PSD in AES ZIP | 937 ms | 922 ms |
| Held-arrow navigation to target preview during 1,120 file operations | 124 ms | 98 ms |

Regular held-arrow preview medians were similar for JPEG and RAW, and P95 rose by roughly 17 ms for each. Target preview P95 during the burst was **169 → 170 ms**. The highest sampled process-tree private commit was **587 → 588 MiB** in the 10,000-JPEG test, **603 → 603 MiB** for RAW, and **585 → 586 MiB** during the burst. Sampling excludes GPU and filesystem caches. Desktop checks sampled no blank frames and found no lingering workers or task temporary files after exit. The build passed 272 unit tests, with 3 ignored.

Fully extract the Windows x64 offline ZIP and double-click `RustView.exe` or `Start-RustView.cmd`. The ZIP includes 7-Zip, LibRaw, ImageMagick, ExifTool, app-local VC++ libraries, and Tesseract with Simplified Chinese and English models. Viewing and OCR work offline from first launch. When moving to a new folder, close RustView and copy `RustView-data` from beside the old executable to keep settings, shortcuts, catalog, and processing records. [Offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.0/RustView-0.7.0-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.7.0/RustView-0.7.0-win-x64-offline.zip.sha256)

[离线版说明 / Offline guide](https://github.com/walker1911/RustView/blob/v0.7.0/OFFLINE.md) · [上个版本 / Previous release](https://github.com/walker1911/RustView/releases/tag/v0.6.9)
