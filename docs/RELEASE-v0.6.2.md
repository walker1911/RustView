# RustView v0.6.2 / 更新说明

## 中文

- **回看近期图片。** 在当前目录或压缩包里往返翻图时，RustView 可复用近期解码的静态图。按 `Alt+O` 打开「选项与快捷键」，可将「最近图片缓存上限」设为 0–2048 MiB。默认使用按本机物理内存计算的推荐值，设为 0 可关闭。修改即时生效，点击「保存选项与快捷键」后可跨启动保留。解码进程和显存各有独立上限。
- **等待时保留清晰画面。** 目标预览就绪前，画面保留上一张的清晰预览，并标明目标文件和暂时显示的文件。目标自己的低清预览就绪后可先行显示，随后切换为清晰图。

原生 Windows 窗口的回看测试使用三张 3840×2560 的 JPEG。缓存上限为 256 MiB 时，四次回看都命中缓存，启动的读取工作进程数为 0。将上限设为 0 后，同样四次操作各产生一次实际读取。原生测试核对了窗口标题和工作进程活动，画面交接由单独的绘制回归覆盖。

下载 Windows x64 完整离线 ZIP，解压到可写文件夹，双击 `RustView.exe` 或 `Start-RustView.cmd`。包内包含 7-Zip、LibRaw、ImageMagick、ExifTool 和 VC++ 运行库，首次启动也可离线。升级前请备份原程序旁的 `RustView-data`，其中有设置、目录库和处理记录。[离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.6.2/RustView-0.6.2-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.6.2/RustView-0.6.2-win-x64-offline.zip.sha256)

## English

- **Revisit recent pictures.** RustView can reuse recently decoded still images when you browse back and forth in the current folder or archive. Open Options with `Alt+O` to adjust the cache limit from 0 to 2048 MiB. The default is based on your computer's physical memory. A limit of 0 disables caching. Changes apply immediately, and saving the options keeps the setting across launches. Decode workers and GPU resources have separate limits.
- **Keep a clear picture while loading.** The previous image's clear overview stays on screen while the target preview loads, with a label identifying both images. A low-resolution preview of the selected target can appear first and give way to the full image once it is ready.

A native Windows test used three 3840×2560 JPEGs. With a 256 MiB limit, all four revisits hit the cache and started zero load workers. With a 0 MiB limit, the same four revisits each started a fresh load. The native test checked window titles and worker activity, and separate drawing regressions covered the visual handoff.

Download the complete Windows x64 offline ZIP, extract it to a writable folder, and double-click `RustView.exe` or `Start-RustView.cmd`. It includes 7-Zip, LibRaw, ImageMagick, ExifTool, and app-local VC++ runtime libraries for offline use from first launch. Before upgrading, back up the `RustView-data` folder beside the old executable to keep your settings, catalog, and processing records. [Offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.6.2/RustView-0.6.2-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.6.2/RustView-0.6.2-win-x64-offline.zip.sha256)

[上个版本 / Previous release](https://github.com/walker1911/RustView/releases/tag/v0.6.1) · [离线版说明 / Offline guide](https://github.com/walker1911/RustView/blob/v0.6.2/OFFLINE.md) · [原生引擎源码 / Native sources](https://github.com/walker1911/RustView/blob/v0.6.2/docs/NATIVE-SOURCES.md) · [验证记录 / Validation](https://github.com/walker1911/RustView/blob/v0.6.2/docs/VALIDATION.md)
