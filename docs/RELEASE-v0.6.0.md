# RustView v0.6.0 / 更新说明

## 中文

v0.6.0 是 Windows x64 预发行版，汇总 v0.5.0 的打开与翻图改进，以及 v0.6.0 的压缩包续看与跳页。

- **先显示图片，再扫描目录。** 直接打开单张本地图片时，先显示所选图片，再在后台扫描同目录。目录扫描完成后，图片加入正常的浏览顺序，画面保持显示。
- **连续翻图。** 翻图和预读都使用当前浏览顺序，切换方向时会优先预读新方向的相邻图片。目标已在读取时可复用任务。同一次解码可先提供小尺寸预览，再送出完整图像。等待时保留上一张的低清画面并显示目标文件名。取消读取或关窗时，正在进行的工作会停止并清理临时文件。
- **压缩包续看。** 重新打开同一个 ZIP、RAR 或 7z 时，回到上次成功显示的图片。位置存入本地目录库。若原条目已找不到，就打开包内第一张可浏览的图片。加密包重开时会再次询问密码。
- **压缩包跳页。** 单图查看时点击工具栏的 **「跳页…」**，输入文件名、完整包内路径或从 1 开始的当前浏览序号。遇到同名图片时可按完整路径选择。

下载 Windows x64 完整离线 ZIP，解压到可写文件夹，双击 `RustView.exe` 或 `Start-RustView.cmd`。7-Zip、LibRaw、ImageMagick、ExifTool 及 VC++ 运行库均随包提供，首次启动可离线运行。更新前关闭旧程序并备份旁边的 `RustView-data`，其中包含设置、目录库和处理记录。[离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.6.0/RustView-0.6.0-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.6.0/RustView-0.6.0-win-x64-offline.zip.sha256)

## English

This Windows x64 prerelease combines the opening and navigation improvements from v0.5.0 with archive resume and jump from v0.6.0.

- **Show the image before scanning its folder.** When you open an individual local image, RustView shows it first and scans its folder in the background. The image stays on screen as the scan places it in the normal browsing order.
- **Continuous navigation.** Navigation and prefetch use the current browsing order. Prefetch follows the direction you are moving, and an active read can serve the matching selection. The same decode can send a small preview before it delivers the full image. While waiting, the viewer keeps a small version of the previous image and labels it with the pending filename. Canceling or closing stops active work and cleans up temporary files.
- **Resume an archive.** Reopen a ZIP, RAR, or 7z archive to return to the last image successfully shown. The local catalog stores the position. If the entry is missing, RustView opens the first browsable image in the archive. Encrypted archives ask for the password again when reopened.
- **Jump within an archive.** In single-image view, click **跳页…** in the toolbar and enter a filename, the full path inside the archive, or a number starting at 1 in the current browsing order. Use the full path to choose among duplicate filenames.

Download the complete Windows x64 offline ZIP, extract it to a writable folder, and double-click `RustView.exe` or `Start-RustView.cmd`. The package includes 7-Zip, LibRaw, ImageMagick, ExifTool, and app-local VC++ runtime libraries for offline use from first launch. Before upgrading, close the previous viewer and back up its `RustView-data` folder, which contains settings, catalog data, and processing records. [Offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.6.0/RustView-0.6.0-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.6.0/RustView-0.6.0-win-x64-offline.zip.sha256)

## 验证与源码 / Validation and sources

Windows x64 单元测试通过 220 项，两项需公开相机样本的测试默认跳过。续看和跳页通过无窗口 Viewer 界面逻辑及真实 Windows 解码进程验证。加密 ZIP、RAR、7z 均能按文件名和序号跳转，并在重开后恢复到 521 MiB 的包内 PSD。加密 RAR 测试还在另一张大图的提取途中关闭，重开时恢复上一张已显示的图片，测试结束后无残留进程或临时文件。

The Windows x64 unit suite passed 220 tests, with two public-camera tests skipped by default. Resume and jump were exercised through the viewer’s interface logic in a headless harness using real Windows decoding workers. Encrypted ZIP, RAR, and 7z tests jumped by filename and number and reopened at a 521 MiB PSD. The RAR test also closed during extraction of a different large image and reopened at the previously displayed image. All observed worker processes exited and the test left no temporary files.

离线 ZIP 已附带 7-Zip 和 LibRaw 对应源码。ImageMagick 和 ExifTool 的对应源码和校验文件由 v0.4.0 Release 提供。7-Zip and LibRaw source ships in the offline ZIP. The v0.4.0 release hosts the matching ImageMagick and ExifTool source archives and checksums. [离线版说明 / Offline guide](https://github.com/walker1911/RustView/blob/v0.6.0/OFFLINE.md) · [原生引擎源码 / Native sources](https://github.com/walker1911/RustView/blob/v0.6.0/docs/NATIVE-SOURCES.md) · [验证记录 / Validation](https://github.com/walker1911/RustView/blob/v0.6.0/docs/VALIDATION.md)
