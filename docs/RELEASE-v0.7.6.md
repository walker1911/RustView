# RustView v0.7.6 · SVG 1.1 兼容修复 / SVG 1.1 compatibility fix

## 简体中文

带有传统 SVG 1.1 `DOCTYPE` 声明的静态 SVG 现在可以正常打开。这也适用于缩略图、放大后补清晰和 ZIP／RAR／7z 内的 SVG。

支持只有外部标识的 `PUBLIC` 和 `SYSTEM` 声明。RustView 跳过外部 DTD 和其他外部资源，并拒绝带内部 DTD 子集的文件。自定义 XML 实体无法展开。SVG 源文件上限为 8 MiB，渲染上限为长边 4,096 像素、总面积 800 万像素。

已复测此前打不开的 SVG，验证窗口显示、缩略图、高清渲染和关窗清理。JPEG、PNG、RAW 与加密压缩包也通过了回归检查。静态 SVG 查看和放大补清晰的介绍见 [v0.7.5 发布说明](https://github.com/walker1911/RustView/releases/tag/v0.7.5)。

### 完整离线版

完整解压 Windows x64 ZIP，双击 `RustView.exe` 或 `Start-RustView.cmd`。运行库、图像与压缩包引擎、Tesseract 及简体中文和英文 OCR 模型均随包提供，首次启动即可离线使用。升级到新目录时，关闭 RustView 后，将旧目录中的 `RustView-data` 复制到新程序旁即可保留数据。[下载完整离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.6/RustView-0.7.6-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.7.6/RustView-0.7.6-win-x64-offline.zip.sha256)

## English

Static SVGs with a legacy SVG 1.1 `DOCTYPE` declaration now open correctly. The fix also applies to thumbnails, high-resolution rendering after zooming, and SVGs inside ZIP, RAR, and 7z archives.

`PUBLIC` and `SYSTEM` declarations with only an external identifier are supported. RustView skips external DTDs and other external resources, rejects files with internal DTD subsets, and prevents expansion of custom XML entities. SVG input is limited to 8 MiB, and renders are capped at 4,096 pixels per edge and 8 million pixels total.

The SVG that originally failed was retested for window display, thumbnails, high-resolution rendering, and cleanup after closing. Regression checks also passed for JPEG, PNG, RAW, and encrypted archives. For background on static SVG viewing and sharper zoom, see the [v0.7.5 release notes](https://github.com/walker1911/RustView/releases/tag/v0.7.5).

### Complete offline version

Extract the entire Windows x64 ZIP and double-click `RustView.exe` or `Start-RustView.cmd`. The bundle includes the runtime libraries, image and archive engines, Tesseract, and Simplified Chinese and English OCR models. It works offline from the first launch. When upgrading to a new folder, close RustView and copy `RustView-data` from the old folder alongside the new executable to keep your data. [Download the complete offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.6/RustView-0.7.6-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.7.6/RustView-0.7.6-win-x64-offline.zip.sha256)
