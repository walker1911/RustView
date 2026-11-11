# RustView v0.4.1

## 中文

v0.4.1 是 Windows x64 公开预发行版，更新了透明图显示和界面字号。

- **透明棋盘格。** 全透明像素露出灰白棋盘格，半透明像素与底色按透明度混合。棋盘格只画在图像范围内，缩放、平移和旋转时格子的大小和位置固定在窗口上。主视图、动画、对比视图、管理侧栏、放大镜和切图预览都使用同样的底纹。底纹只用于显示。
- **界面字号。** 正文和按钮为 14，辅助信息和等宽文字为 13，面板与对话框标题为 17，欢迎页标题为 24。这些是界面逻辑字号，系统显示比例决定屏幕上的实际大小。路径和状态信息太长时显示省略号，悬停可看全文。

完整解压后双击 `RustView.exe` 或 `Start-RustView.cmd`。离线 ZIP 包含 7-Zip、LibRaw、ImageMagick、ExifTool 和 VC++ 运行库，首次启动无需联网或另装依赖。[下载离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.4.1/RustView-0.4.1-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.4.1/RustView-0.4.1-win-x64-offline.zip.sha256)

## English

v0.4.1 is a public prerelease for Windows x64 with a transparency backdrop and more consistent interface text sizes.

- **Transparency checkerboard.** Fully transparent pixels reveal a gray and white checkerboard, and semitransparent pixels blend with it using their alpha values. The checkerboard is clipped to the image and stays fixed in size and position as you zoom, pan, or rotate. It appears in the main view, animation, comparison, management sidebar preview, magnifier, and previews between images. The pattern is for display only.
- **Interface typography.** Body text and buttons use 14 points, secondary and monospaced text use 13, panel and dialog headings use 17, and the welcome title uses 24. These are logical UI sizes that follow system display scaling. Long paths and status messages truncate with an ellipsis and show their full text on hover.

Fully extract the ZIP and run `RustView.exe` or `Start-RustView.cmd`. The archive includes 7-Zip, LibRaw, ImageMagick, ExifTool, and the VC++ runtime libraries. It works offline from first launch. [Download offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.4.1/RustView-0.4.1-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.4.1/RustView-0.4.1-win-x64-offline.zip.sha256)

## Sources and validation / 源码与验证

The runtime ZIP contains 7-Zip source and the official LibRaw source and binary archive. The v0.4.0 Release hosts the matching ImageMagick and ExifTool sources. 7-Zip 和 LibRaw 源码随运行包提供，ImageMagick 和 ExifTool 的对应源码可从 v0.4.0 Release 下载。[离线版说明 / Offline guide](../OFFLINE.md) · [原生引擎源码 / Native sources](NATIVE-SOURCES.md) · [验证记录 / Validation](VALIDATION.md)


Windows x64 validation passed 209 unit tests and 87 checks from a fresh offline extraction. 新版通过 209 项单测和 87 项离线解压回归。两项公开相机单测默认跳过。
