# RustView v0.6.9 · 图上选字与超大图浏览 / Select image text and browse huge images

## 中文

- **直接在图片上选字。** 在静态图上停留约 650 毫秒，等前台读图空闲后，RustView 会在后台离线识字。鼠标移到文字上出现文本光标，即可跨字或跨行拖选，按 `Ctrl+C` 复制。在文字上或已有选区时，`Ctrl+A` 选取当前图中识别到的全部文字。单击空白处或按 `Esc` 清除选区，空白处仍可拖动平移。整图和框选 OCR 继续可从右键菜单调用。随包提供简体中文和英文模型。
- **缩放、平移超大本地 TIFF／BigTIFF。** 符合条件的分块金字塔图像先显示内嵌全景，再按视野读取细节。细节加载期间，同一区域保留较低清晰度的画面。超出常规预算的本地文件进入只读查看模式，要求有分块的原尺寸层和长边不超过 2048 像素的低清层。支持常见无符号 8／16 位 RGB、RGBA、灰度和 JPEG 压缩的 8 位 YCbCr。此模式可用滚轮缩放、拖动平移，按 `/` 查看 1:1。[兼容范围](https://github.com/walker1911/RustView/blob/v0.6.9/docs/COMPATIBILITY.md)
- **修复 PSD／PSB 合成图读取。** 跳过图层数据，读取保存的合成图。ZIP 压缩的合成图，包括带预测编码的文件，可在受限工作进程内逐行解压。复杂文档的完整合成图缺失或不可用时，可显示已有的 Photoshop 内嵌 JPEG，并在画面上标为「PSD 内嵌预览」。PSD／PSB 源文件保持只读，图像仍受现有像素内存预算约束。[PSD／PSB 支持范围](https://github.com/walker1911/RustView/blob/v0.6.9/docs/PSD.md)
- **区分点按和长按翻图。** 点按优先读取完整图，缓存命中可直接出清晰图。小图可以跳过低清过渡。较大的本地 PSD／PSB 在完整读取期间可先显示目标自己的内嵌预览。长按仍优先给出可用的目标低清，松手后读取最终选中的清晰图。等待新图有可用像素时，界面保留上一张的清晰概览，并标出正在读取的文件。[浏览说明](https://github.com/walker1911/RustView/blob/v0.6.9/docs/PICASA-BROWSING.md)

### Windows 实测

- **图上选字**覆盖透明底浅色中文、英文、跨行选区、复制粘贴、空白处平移、翻图时取消和识字中关闭窗口。另在万张 JPEG 目录和相机 RAW 样本中对照仅手动 OCR 与自动 OCR 的开发版，采样中未观察到 OCR 在首图前或长按翻图中启动。
- **超大 BigTIFF**使用由 ESO VISTA 银河系原图转换的 108,199 × 81,503 分块 BigTIFF（34.13 GiB）。开发版的三次 Windows 桌面测试中，启动到全景可见的中位数为 667 毫秒，进程树私有提交内存采样峰值最高约 486 MiB。平移和滚轮缩放的采样中未观察到空白画面。
- **PSD 和翻图**回归覆盖 RAW、RLE、ZIP、ZIP 预测编码、内嵌预览回退，以及加密 7z 中的合成图。另在 Windows 桌面用同一组样本对照翻图改动前后的开发版。小 JPEG 的低清过渡从三次出现降为零次，清晰图出现的中位数从 206 毫秒到 169 毫秒。所测大 PSD 的目标预览出现中位数从 88 毫秒到 24 毫秒。这些数据来自特定样本和电脑。

完整解压 Windows x64 离线 ZIP，再双击 `RustView.exe` 或 `Start-RustView.cmd`。随包提供 7-Zip、LibRaw、ImageMagick、ExifTool、VC++ 运行库和 Tesseract 简体中文及英文模型。首次启动即可离线看图和识字。迁移到新目录时，关闭 RustView 后复制旧程序旁的 `RustView-data`，即可带上设置、快捷键、目录库和处理记录。[离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.6.9/RustView-0.6.9-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.6.9/RustView-0.6.9-win-x64-offline.zip.sha256)

## English

- **Select text directly on a picture.** After the picture has been visible for about 650 ms and foreground loading is idle, RustView runs offline OCR in the background. When the text cursor appears, drag across characters or lines and press `Ctrl+C`. With the pointer over text or an active selection, `Ctrl+A` selects all recognized text in the picture. Click empty space or press `Esc` to clear the selection. Dragging empty space still pans the picture. Whole-image and selected-area OCR remain in the right-click menu. Simplified Chinese and English models come with the viewer.
- **Zoom and pan huge local TIFF/BigTIFF files.** Supported tiled pyramids open at the embedded overview and read visible detail tiles as you browse. The same area stays visible at lower resolution while details load. The read-only path for files above the usual budget requires a tiled full-resolution level and a reduced overview no longer than 2,048 pixels on its longest edge. It supports common unsigned 8- and 16-bit RGB, RGBA, and grayscale, plus JPEG-compressed 8-bit YCbCr. Use the wheel to zoom, drag to pan, or press `/` for 1:1. [Compatibility](https://github.com/walker1911/RustView/blob/v0.6.9/docs/COMPATIBILITY.md)
- **Fix PSD/PSB composite decoding.** RustView skips the layers and reads the saved composite. ZIP composites, including predicted ZIP, can decode row by row in a bounded worker. When a complex document has no usable full composite, RustView can show its embedded Photoshop JPEG with a **PSD 内嵌预览** (PSD embedded preview) label. PSD/PSB source files remain read-only and the existing decoded-pixel limits still apply. [PSD/PSB support](https://github.com/walker1911/RustView/blob/v0.6.9/docs/PSD.md)
- **Refine tap and hold navigation.** A tap prioritizes the full image. Cached images display immediately and smaller images can skip the low-resolution transition. A large local PSD/PSB can show its own embedded thumbnail while the full composite loads. Holding an arrow key still prioritizes available target previews and loads the final selection in full after release. The previous clear overview stays visible, with a loading label, until the target has pixels to show. [Browsing guide](https://github.com/walker1911/RustView/blob/v0.6.9/docs/PICASA-BROWSING.md)

### Windows validation

- **On-image text selection** covered light Chinese text on a transparent background, English, multiline selection, clipboard paste, panning from empty space, cancellation while navigating, and closing during OCR. A separate development-build comparison with a 10,000-JPEG folder and camera RAW files observed no automatic OCR process launches before the first image or during held-arrow navigation.
- **Huge BigTIFF** testing used a 108,199 × 81,503 tiled BigTIFF (34.13 GiB) converted from the original ESO VISTA Milky Way image. Across three Windows desktop runs of the development build, the panorama appeared in 667 ms from process launch at the median and sampled process-tree private commit peaked at about 486 MiB. Panning and wheel-zoom captures sampled no blank frames.
- **PSD and navigation** regressions covered raw, RLE, ZIP, ZIP prediction, the embedded preview fallback, and a composite inside encrypted 7z. In a Windows desktop comparison of development builds before and after the navigation changes, one small JPEG showed a low-resolution frame in three trials before the change and zero afterward. Its median time to the clear image moved from 206 ms to 169 ms. For the large PSD sample, median time to its own preview moved from 88 ms to 24 ms. These measurements reflect the tested files and PC.

Fully extract the Windows x64 offline ZIP and double-click `RustView.exe` or `Start-RustView.cmd`. The ZIP includes 7-Zip, LibRaw, ImageMagick, ExifTool, app-local VC++ libraries, and Tesseract with Simplified Chinese and English models. Viewing and text recognition work offline from the first launch. When moving to a new folder, close RustView and copy `RustView-data` from beside the old executable to carry over your settings, shortcuts, catalog, and processing records. [Offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.6.9/RustView-0.6.9-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.6.9/RustView-0.6.9-win-x64-offline.zip.sha256)

[离线版说明 / Offline guide](https://github.com/walker1911/RustView/blob/v0.6.9/OFFLINE.md) · [上个版本 / Previous release](https://github.com/walker1911/RustView/releases/tag/v0.6.8)
