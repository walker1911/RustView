# RustView offline edition / 完整离线版

## 中文

本文件随 `RustView-0.4.0-win-x64-offline.zip` 发行。这个 Windows x64 包已经包含看图所需的 7-Zip、LibRaw、ImageMagick、ExifTool 及 Microsoft VC++ 运行库。

1. 完整解压到可以写入的文件夹。
2. 双击 `RustView.exe`，或运行 `Start-RustView.cmd`。
3. 打开图片、文件夹或 ZIP／RAR／7z。首次启动也不需要联网或安装依赖。

保留整个目录，尤其是 `native`、`licenses` 和 `sources`；不要只复制一个 EXE。设置、目录库和处理记录会保存在程序旁的 `RustView-data` 中。启动器直接运行程序，不调用下载脚本，不修改 PATH 或文件关联。可选双击 `Setup-File-Associations.cmd` 注册图片候选关联及 ZIP／RAR／7z 右键入口，再由 Windows 默认应用确认图片默认程序；移除用 `Remove-File-Associations.cmd`。注册不是离线看图的前提，详见[文件关联说明](docs/WINDOWS-INTEGRATION.md)。操作系统和显卡驱动仍由 Windows 提供。

第三方源码与构建说明见[源码说明](docs/NATIVE-SOURCES.md)（离线包中也保存在 `sources/README.md`）以及同一 [v0.4.0 Release](https://github.com/walker1911/RustView/releases/tag/v0.4.0) 的源码附件。普通使用者无需下载源码附件即可离线看图。原生引擎未升级，ExifTool 对应源码附件沿用 `RustView-0.3.0-exiftool-sources.zip` 历史文件名；ImageMagick 对应源码为 `ImageMagick-7.1.2-35-Windows.7z`。应用代码为 MIT 许可，各引擎及其依赖保留各自许可。

## English

This file accompanies `RustView-0.4.0-win-x64-offline.zip`. The Windows x64 package includes 7-Zip, LibRaw, ImageMagick, ExifTool, and app-local Microsoft VC++ runtime libraries.

1. Fully extract the ZIP into a writable folder.
2. Double-click `RustView.exe`, or run `Start-RustView.cmd`.
3. Open a picture, folder, or ZIP / RAR / 7z archive. No internet connection or dependency installation is needed, including on first launch.

Keep the entire folder, including `native`, `licenses`, and `sources`. Settings, catalog data, and processing records are stored in `RustView-data` beside the executable. The launcher starts the viewer directly and performs no downloads or changes to PATH or file associations. Optionally run `Setup-File-Associations.cmd` for image candidates and ZIP/RAR/7z context-menu entries, then confirm image defaults in Windows Default Apps. Use `Remove-File-Associations.cmd` to remove registration. Registration is not required for viewing. See [Windows integration](docs/WINDOWS-INTEGRATION.md). Windows and graphics drivers remain system prerequisites.

See the [source guide](docs/NATIVE-SOURCES.md), also copied to `sources/README.md` in the offline package, and the source attachments on the same [v0.4.0 Release](https://github.com/walker1911/RustView/releases/tag/v0.4.0) for third-party sources and rebuild instructions. These attachments are optional for ordinary offline viewing. Native engine versions are unchanged: the ExifTool source attachment retains its historical `RustView-0.3.0-exiftool-sources.zip` name; the ImageMagick source attachment is `ImageMagick-7.1.2-35-Windows.7z`. Application code is MIT licensed; native engines and their dependencies retain their separate licenses.
