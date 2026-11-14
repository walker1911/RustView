# RustView v0.7.1 · 首图之后更早翻图 / Start browsing sooner after the first photo

## 中文

- **双击本地图片，先出首图。** 首图绘制后，后台扫描仍要遍历目录里的文件名，才能确认自然排序中的邻图。遍历结束会先交付前后各最多 8 张图片，让你在全目录排序和索引完成前开始翻图。
- **首图刚显示就按左／右键也有效。** 邻图还没准备好时会记住最近一次翻图意图。目标可用后再执行，保持正确顺序。完整目录接上后可继续翻过首批邻图。

### 同一万张 JPEG 目录对照

在同一台 Windows 电脑上，从目录中间的同一张图片启动独立进程，用窗口标题和桌面像素分别核对目标身份、可见画面和清晰图。左右方向都测。新配置指程序旁的应用数据为空，复用配置包含已有的本地目录库。两组都共用预热后的 Windows 文件缓存和 GPU 缓存，每版每种测试与配置各排除一次热身，再测 4 次。

单次按键测试在首图清晰时只按一次左键或右键。另一个探测测试约每 32 毫秒点按一次，直到窗口标题变化。表中时间均为中位数，配对差与两组中位数之差可能不同。

| 测试项目 | 应用配置 | v0.7.0 | v0.7.1 |
|---|---|---:|---:|
| 单次按键测试，启动到首图清晰 | 新配置 | 731 ms | 728 ms |
| 单次按键测试，启动到首图清晰 | 复用配置 | 695 ms | 691 ms |
| 首图清晰后只按一次，成功翻图 | 两类合计 | 0／8 | 8／8 |
| 单次按键后，目标首次可见 | 新配置／复用配置 | — | 167／162 ms |
| 单次按键后，目标清晰 | 新配置／复用配置 | — | 333／337 ms |
| 探测测试，首图清晰到目标首次可见 | 新配置／复用配置 | 199／199 ms | 167／163 ms |
| 探测测试，首图清晰到目标清晰 | 新配置／复用配置 | 353／358 ms | 329／292 ms |

在探测测试中，新配置和复用配置的首次有效按键上界均从约 **34 ms 缩短到第一下**。这个上界来自实际发出的按键及后续标题变化。单次按键对照中首图耗时接近，探测对照中首图中位数为新配置 **723 → 724 ms**、复用配置 **696 → 715 ms**。

提前开始读取邻图增加了采样内存。单次按键测试的进程树私有提交峰值中位数在两类配置均增加约 **19–20 MiB**，新版实际翻到了邻图。两版均成功切图的探测测试中，峰值中位数为新配置 **518 → 535 MiB**、复用配置 **537 → 547 MiB**，两类配置合计的最高采样值为 **558 → 566 MiB**。采样约每 90 ms 一次，统计应用及工作进程的私有提交内存，GPU 和系统缓存另计。

额外 6 次快速反向测试均通过，随后正向翻过首批邻图继续浏览。测试未采到空白画面。关闭后未发现残留工作进程或任务临时文件。单元测试 276 项通过，3 项跳过。

完整解压 Windows x64 离线 ZIP，再双击 `RustView.exe` 或 `Start-RustView.cmd`。包内提供 7-Zip、LibRaw、ImageMagick、ExifTool、VC++ 运行库和 Tesseract 简体中文及英文模型，首次启动即可离线看图和识字。迁移到新目录时，关闭 RustView 后复制旧程序旁的 `RustView-data`，即可带上设置、快捷键、目录库和处理记录。[离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.1/RustView-0.7.1-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.7.1/RustView-0.7.1-win-x64-offline.zip.sha256)

## English

- **Double-click a local photo and see it first.** The background scan still visits the folder's filenames to find the correct neighbors in natural order. Once it has enumerated them, RustView makes up to eight images on either side available for browsing while the full directory sort and index finish.
- **An early Left or Right press still works.** RustView saves the latest navigation request while it is finding the neighboring photos and applies it when the target becomes available. The full list then lets you continue past the first group.

### Paired tests in the same 10,000-JPEG folder

Each run started a new process at the same photo near the middle of the folder on one Windows PC. Window titles and desktop pixels verified the selected file, visible image, and clear texture. Both directions were tested. A fresh profile started with empty application data beside the executable, and a reused profile kept an existing catalog. Both used warm Windows filesystem and GPU caches. One warmup was excluded for each version, mode, and profile before four measured runs.

The single-tap test sent one Left or Right press as soon as the first photo was clear. A separate probe sent a discrete tap about every 32 ms until the window title changed. Times in the table are medians.

| Measurement | App profile | v0.7.0 | v0.7.1 |
|---|---|---:|---:|
| Single-tap run, launch to first clear photo | Fresh | 731 ms | 728 ms |
| Single-tap run, launch to first clear photo | Reused | 695 ms | 691 ms |
| Successful navigation after just one early tap | Both | 0/8 | 8/8 |
| Single tap to visible target | Fresh / reused | — | 167 / 162 ms |
| Single tap to clear target | Fresh / reused | — | 333 / 337 ms |
| Probe, first clear photo to visible target | Fresh / reused | 199 / 199 ms | 167 / 163 ms |
| Probe, first clear photo to clear target | Fresh / reused | 353 / 358 ms | 329 / 292 ms |

In the probe, the upper bound on an accepted tap moved from about **34 ms to the first tap** for both profiles. It is derived from the injected tap and subsequent title change. First-photo timing stayed close in the single-tap comparison. In the probe, first-photo medians were **723 → 724 ms** with a fresh profile and **696 → 715 ms** with a reused profile.

Starting the next read sooner raised sampled memory. Median peak process-tree private commit rose by about **19–20 MiB** in the single-tap tests, in which v0.7.1 reached the neighboring photo. In the probe, both versions navigated successfully. The median peaks were **518 → 535 MiB** for a fresh profile and **537 → 547 MiB** for a reused profile. The highest sampled value across profiles was **558 → 566 MiB**. Samples taken roughly every 90 ms cover the application and worker processes' private commit. GPU and system caches are outside this measurement.

Six additional Windows trials passed rapid direction reversal followed by navigation beyond the first group. The tests sampled no blank frames and found no surviving workers or task temporary files after exit. 276 unit tests passed and 3 were ignored.

Fully extract the Windows x64 offline ZIP and double-click `RustView.exe` or `Start-RustView.cmd`. The ZIP includes 7-Zip, LibRaw, ImageMagick, ExifTool, app-local VC++ libraries, and Tesseract with Simplified Chinese and English models. Viewing and OCR work offline from the first launch. When moving to a new folder, close RustView and copy `RustView-data` from beside the old executable to carry over settings, shortcuts, the catalog, and processing records. [Offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.1/RustView-0.7.1-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.7.1/RustView-0.7.1-win-x64-offline.zip.sha256)

[离线版说明 / Offline guide](https://github.com/walker1911/RustView/blob/v0.7.1/OFFLINE.md) · [上个版本 / Previous release](https://github.com/walker1911/RustView/releases/tag/v0.7.0)
