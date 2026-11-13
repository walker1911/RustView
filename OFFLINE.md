# RustView offline edition / 完整离线版

## 中文

本文件随 `RustView-0.6.7-win-x64-offline.zip` 发行。这个 Windows x64 包已经包含看图所需的 7-Zip、LibRaw、ImageMagick、ExifTool 及 Microsoft VC++ 运行库。[离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.6.7/RustView-0.6.7-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.6.7/RustView-0.6.7-win-x64-offline.zip.sha256)

1. 完整解压到可以写入的文件夹。
2. 双击 `RustView.exe`，或运行 `Start-RustView.cmd`。
3. 打开图片、文件夹或 ZIP／RAR／7z。首次启动也不需要联网或安装依赖。

保留整个目录，尤其是 `native`、`licenses` 和 `sources`；不要只复制一个 EXE。设置、目录库和处理记录会保存在程序旁的 `RustView-data` 中。启动器直接运行程序，不调用下载脚本，不修改 PATH 或文件关联。可选双击 `Setup-File-Associations.cmd` 注册图片候选关联及 ZIP／RAR／7z 右键入口，再由 Windows 默认应用确认图片默认程序；移除用 `Remove-File-Associations.cmd`。注册不是离线看图的前提，详见[文件关联说明](docs/WINDOWS-INTEGRATION.md)。操作系统和显卡驱动仍由 Windows 提供。

7-Zip 源码和 LibRaw 官方源码与二进制归档随 ZIP 提供。v0.6.7 的 ImageMagick 和 ExifTool 对应源码可从 v0.4.0 Release 下载。源码用于查看和重编，日常看图只需离线 ZIP。构建说明也保存在包内 `sources/README.md`，各引擎的许可随包提供。[源码说明](docs/NATIVE-SOURCES.md) · [ImageMagick 源码](https://github.com/walker1911/RustView/releases/download/v0.4.0/ImageMagick-7.1.2-35-Windows.7z) · [ExifTool 源码](https://github.com/walker1911/RustView/releases/download/v0.4.0/RustView-0.3.0-exiftool-sources.zip)

## English

This file accompanies `RustView-0.6.7-win-x64-offline.zip`. The Windows x64 package includes 7-Zip, LibRaw, ImageMagick, ExifTool, and app-local Microsoft VC++ runtime libraries. [Offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.6.7/RustView-0.6.7-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.6.7/RustView-0.6.7-win-x64-offline.zip.sha256)

1. Fully extract the ZIP into a writable folder.
2. Double-click `RustView.exe`, or run `Start-RustView.cmd`.
3. Open a picture, folder, or ZIP / RAR / 7z archive. No internet connection or dependency installation is needed, including on first launch.

Keep the entire folder, including `native`, `licenses`, and `sources`. Settings, catalog data, and processing records are stored in `RustView-data` beside the executable. The launcher starts the viewer directly and performs no downloads or changes to PATH or file associations. Optionally run `Setup-File-Associations.cmd` for image candidates and ZIP/RAR/7z context-menu entries, then confirm image defaults in Windows Default Apps. Use `Remove-File-Associations.cmd` to remove registration. Registration is not required for viewing. See [Windows integration](docs/WINDOWS-INTEGRATION.md). Windows and graphics drivers remain system prerequisites.

The ZIP includes 7-Zip source and the official LibRaw source and binary archive. The corresponding sources for ImageMagick and ExifTool in v0.6.7 are available from the v0.4.0 Release. The ZIP is the only download needed for viewing. Rebuild instructions are also included in `sources/README.md` and engine licenses ship with the package. [Source guide](docs/NATIVE-SOURCES.md) · [ImageMagick source](https://github.com/walker1911/RustView/releases/download/v0.4.0/ImageMagick-7.1.2-35-Windows.7z) · [ExifTool source](https://github.com/walker1911/RustView/releases/download/v0.4.0/RustView-0.3.0-exiftool-sources.zip)
