# RustView v0.7.3 · 双击图片启动优化 / Faster direct image opening

## 中文

- **双击图片时更早显示画面。** Windows 上直接打开静态图时优先尝试符合条件的硬件 OpenGL。若不可用，依次尝试 Vulkan 和默认渲染方式。
- GitHub 项目首页默认使用简体中文，可从首页切换到英文说明。

### 双击图片与直接打开

以下均为同一台 Windows 电脑上测得的首图可见中位数。同一 JPEG 分别从 OneDrive 桌面和本地测试目录经已安装的 Windows 文件关联打开，每版、每处各 5 次。JPEG／PNG 配对测试用单独进程直接打开相同编码字节的文件，执行与普通文件关联一致的启动参数。每版、每种格式、每处各取 6 次，首轮单独记录。

| 打开方式 | v0.7.2 | v0.7.3 |
|---|---:|---:|
| Windows 文件关联，OneDrive 桌面 JPEG | 729 ms | 517 ms |
| Windows 文件关联，本地目录 JPEG | 685 ms | 551 ms |
| 直接打开，OneDrive 桌面 JPEG | 618 ms | 472 ms |
| 直接打开，本地目录 JPEG | 656 ms | 445 ms |
| 直接打开，OneDrive 桌面 PNG | 651 ms | 436 ms |
| 直接打开，本地目录 PNG | 666 ms | 423 ms |

测试使用预热过的系统缓存。v0.7.3 首轮直接打开的OneDrive 桌面 JPEG、本地 JPEG、本地 PNG 分别为 1.01、1.18、1.06 秒。后续计入中位数的本地 JPEG 仍有一轮约 1.08 秒。

### 翻图和内存

每版分别在万张 JPEG 目录和相机 RAW 样本上运行 8 次长按翻图。表中均为中位数。进程树私有提交内存约每 90 毫秒采样，统计应用和工作进程，GPU 内存与系统文件缓存另计。

| 长按翻图测试 | v0.7.2 | v0.7.3 |
|---|---:|---:|
| 万张 JPEG，启动到清晰首图 | 720 ms | 791 ms |
| 万张 JPEG，按键到目标预览 | 70 ms | 20 ms |
| 万张 JPEG，松键到清晰图 | 78 ms | 32 ms |
| 万张 JPEG，点按回看缓存到清晰图 | 75 ms | 49 ms |
| 万张 JPEG，进程树采样峰值 | 586 MiB | 409 MiB |
| 相机 RAW，启动到清晰首图 | 731 ms | 811 ms |
| 相机 RAW，按键到目标预览 | 65 ms | 20 ms |
| 相机 RAW，松键到清晰图 | 341 ms | 312 ms |
| 相机 RAW，进程树采样峰值 | 568 MiB | 388 MiB |

这两组长按测试的首图清晰时间中位数延长约 71–80 毫秒。另一组首图刚清晰时点按一次方向键的万张 JPEG 测试中，两版均成功 8／8 次，新配置和复用配置各 4 次。点按到邻图清晰的中位数分别从 292 降到 266 毫秒、从 291 降到 262 毫秒。翻图测试未采到空白帧，透明 PNG 的棋盘格和大 BigTIFF 的分块缩放也已复测。通过 277 项单元测试，3 项跳过。

完整解压 Windows x64 离线 ZIP，再双击 `RustView.exe` 或 `Start-RustView.cmd`。随包提供 7-Zip、LibRaw、ImageMagick、ExifTool、VC++ 运行库、Tesseract 及其简体中文和英文模型，首次启动即可离线看图和识字。迁移到新目录时，关闭 RustView 后复制旧程序旁的 `RustView-data`，可保留设置、快捷键、目录库和处理记录。[离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.3/RustView-0.7.3-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.7.3/RustView-0.7.3-win-x64-offline.zip.sha256)

## English

- **See the picture sooner after a double-click.** When directly opening a still image on Windows, RustView tries eligible hardware OpenGL first. It can fall back to Vulkan and then the default renderer.
- The GitHub homepage opens in Simplified Chinese, with a link to the English README.

### File association and direct opening

All times are medians to the first visible image on the same Windows PC. The installed Windows file association opened the same JPEG from the OneDrive desktop and a local test folder five times each per version. Paired direct-open tests used new processes, identical encoded image bytes, and the normal file association's launch arguments. Six runs per version, format, and location contributed to the direct-open medians. The initial trial was recorded separately.

| Opening method | v0.7.2 | v0.7.3 |
|---|---:|---:|
| Windows file association, OneDrive desktop JPEG | 729 ms | 517 ms |
| Windows file association, local-folder JPEG | 685 ms | 551 ms |
| Direct open, OneDrive desktop JPEG | 618 ms | 472 ms |
| Direct open, local-folder JPEG | 656 ms | 445 ms |
| Direct open, OneDrive desktop PNG | 651 ms | 436 ms |
| Direct open, local-folder PNG | 666 ms | 423 ms |

The tests used warm system caches. Initial v0.7.3 direct-open trials took 1.01 seconds for the OneDrive desktop JPEG, 1.18 seconds for the local JPEG, and 1.06 seconds for the local PNG. One subsequent local JPEG run included in the median still took about 1.08 seconds.

### Browsing and memory

Each version ran eight held-arrow trials in a 10,000-JPEG folder and another eight with camera RAW samples. The table shows medians. Process-tree private commit was sampled about every 90 ms and covers the app and its workers. GPU memory and the system file cache are outside that measurement.

| Held-arrow workload | v0.7.2 | v0.7.3 |
|---|---:|---:|
| 10,000 JPEGs, launch to first clear image | 720 ms | 791 ms |
| 10,000 JPEGs, key press to target preview | 70 ms | 20 ms |
| 10,000 JPEGs, key release to clear image | 78 ms | 32 ms |
| 10,000 JPEGs, cached tap to clear image | 75 ms | 49 ms |
| 10,000 JPEGs, sampled process-tree peak | 586 MiB | 409 MiB |
| Camera RAW, launch to first clear image | 731 ms | 811 ms |
| Camera RAW, key press to target preview | 65 ms | 20 ms |
| Camera RAW, key release to clear image | 341 ms | 312 ms |
| Camera RAW, sampled process-tree peak | 568 MiB | 388 MiB |

These held-arrow workloads took about 71–80 ms longer to show the first clear image at the median. In the separate single-tap 10,000-JPEG test, both versions succeeded on eight of eight runs, four each with fresh and reused app profiles. Median tap-to-clear fell from 292 to 266 ms with a fresh profile and from 291 to 262 ms with a reused profile. The browsing tests sampled no blank frames. Transparent PNG checkerboard alignment and tiled zoom on a large BigTIFF also passed. 277 unit tests passed, with 3 ignored.

Fully extract the Windows x64 offline ZIP and double-click `RustView.exe` or `Start-RustView.cmd`. The ZIP includes 7-Zip, LibRaw, ImageMagick, ExifTool, app-local VC++ libraries, and Tesseract with Simplified Chinese and English models. Viewing and OCR work offline from the first launch. When moving to a new folder, close RustView and copy `RustView-data` from beside the old executable to keep settings, shortcuts, the catalog, and processing records. [Offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.3/RustView-0.7.3-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.7.3/RustView-0.7.3-win-x64-offline.zip.sha256)

[离线版说明 / Offline guide](https://github.com/walker1911/RustView/blob/v0.7.3/OFFLINE.md) · [上个版本 / Previous release](https://github.com/walker1911/RustView/releases/tag/v0.7.2)
