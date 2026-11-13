# RustView v0.6.4 / 更新说明

## 中文

- **双击图片更快出图。** 直接打开图片时优先初始化 Vulkan，并跳过这个窗口首次出现时的系统过渡动画。Vulkan 初始化失败时会回退到常规图形启动流程，系统动画设置保持原样。
- **文件 → 最近打开…** 可重新打开最近成功打开的图片、文件夹和压缩包，最多显示 20 项。压缩包条目显示上次浏览位置，重开后可续看。历史保存在程序旁的本地目录库，加密包重新打开时仍需输入密码。

### Windows x64 实测

在同一台 Windows 电脑上，分别用 v0.6.3 和 v0.6.4 离线包启动新进程，打开万张图库中的一张 3840 × 2560 JPEG。两版各测 10 次，暖机轮次不计入结果。计时取自实际屏幕像素，使用系统正常的文件和驱动缓存。

| 指标 | v0.6.3 | v0.6.4 |
|---|---:|---:|
| 启动到首图可见，中位数 | 927 ms | 611 ms |
| 启动到首图可见，P95 | 990 ms | 642 ms |
| 启动到清晰画面可见，中位数 | 937 ms | 620 ms |

另外在测试进程中模拟 Vulkan 驱动不可用，两次启动均通过回退路径显示图片并正常退出。原生窗口验证覆盖最近打开的图片、空文件夹和加密 7z，以及重开加密包后的密码提示与续看。通过 231 项单测、79 项离线运行检查和 8 组加密 RAR 翻图、取消及清理检查。

下载 Windows x64 完整离线 ZIP，完整解压后双击 `RustView.exe` 或 `Start-RustView.cmd`。包内提供 7-Zip、LibRaw、ImageMagick、ExifTool 和 VC++ 运行库，首次启动可离线。迁移到新目录时，可在关闭 RustView 后复制旧程序旁的 `RustView-data`，保留已有设置、目录库和处理记录。[离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.6.4/RustView-0.6.4-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.6.4/RustView-0.6.4-win-x64-offline.zip.sha256)

## English

- **Get to the first picture sooner.** Launching RustView with an image tries Vulkan first and skips the opening transition for that window. If Vulkan initialization fails, RustView uses the regular graphics startup path. Windows animation settings are untouched.
- **File → Recent** reopens up to 20 recently opened images, folders, and archives. An archive shows the last viewed entry and can reopen at that position. History stays in the local catalog beside the executable. Encrypted archives prompt for the password when reopened.

### Windows x64 measurements

On the same Windows PC, fresh v0.6.3 and v0.6.4 processes opened a 3840 × 2560 JPEG in a 10,000-image folder. Each offline build had 10 timed launches after warmup. The timings come from actual screen pixels with normal Windows file and driver caching.

| Measurement | v0.6.3 | v0.6.4 |
|---|---:|---:|
| Launch to first visible image, median | 927 ms | 611 ms |
| Launch to first visible image, P95 | 990 ms | 642 ms |
| Launch to clear image at fit zoom, median | 937 ms | 620 ms |

Two additional launches with Vulkan driver discovery disabled in the test process showed the image via the fallback path and exited cleanly. Native-window checks covered recent images, an empty folder, and an encrypted 7z archive, including the password prompt and resume position on reopen. The build passed 231 unit tests, 79 offline runtime checks, and eight encrypted-RAR navigation, cancellation, and cleanup checks.

Download the complete Windows x64 offline ZIP, extract it fully, and double-click `RustView.exe` or `Start-RustView.cmd`. The package includes 7-Zip, LibRaw, ImageMagick, ExifTool, and app-local VC++ runtime libraries for offline use from first launch. When moving to a new folder, close RustView and copy `RustView-data` from beside the previous executable to carry over your settings, catalog, and processing records. [Offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.6.4/RustView-0.6.4-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.6.4/RustView-0.6.4-win-x64-offline.zip.sha256)

[上个版本 / Previous release](https://github.com/walker1911/RustView/releases/tag/v0.6.3) · [离线版说明 / Offline guide](https://github.com/walker1911/RustView/blob/v0.6.4/OFFLINE.md) · [原生引擎源码 / Native sources](https://github.com/walker1911/RustView/blob/v0.6.4/docs/NATIVE-SOURCES.md)
