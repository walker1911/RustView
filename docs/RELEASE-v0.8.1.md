# RustView v0.8.1 · RAW 首图与 A/B 分割对照

[English](#english)

## 简体中文

### RAW 首图先绘制清晰帧

直接打开 RAW 时，RustView 先绘制首张清晰图，再初始化字体和图库。RustView 分块读回解码后的像素，减少整块缓冲区的重复写入。启动跟踪将清晰帧绘制与界面初始化分别记录。

### 拖动分割线比较 A 和 B

在「管理」中选中至少两张图，按 **Ctrl+Alt+R** 打开比较。选好参考图后，点「固定参考图 A」，再点「分割对照」。拖动中线改变左右两侧的显示范围，A 和 B 共用画布、缩放与平移。左右键切换 B。按住「按住看参考图 A」可暂时看完整的 A，松手回到分割对照。

原生窗口实测使用同尺寸 PSD 和透明 PNG。在 1:1 下拖动分割线、按住查看 A 及快速切换 B 时，标记保持像素对齐，透明区域正确显示棋盘格。

### RAW 默认浏览

默认浏览优先显示相机内嵌的可用预览。两版在同一台 Windows 电脑上使用同一套离线运行库，交替启动新进程。四种 RAW 各测 6 对，共 24 对，预热样本不计入统计。计时从启动到原生窗口中采到首张清晰图。采样同时识别胶片带出现前后的清晰画面。P50 为中位数，P95 为第 95 百分位。

| 样本 | v0.8.0 P50 / P95（ms） | v0.8.1 P50 / P95（ms） |
| --- | ---: | ---: |
| Nikon Z6 NEF | 409.47 / 457.48 | 403.39 / 406.74 |
| Canon EOS R CR3 | 457.21 / 472.15 | 448.95 / 455.70 |
| Sony a7 III ARW | 336.93 / 373.14 | 338.41 / 342.04 |
| iPhone 12 Pro DNG | 433.49 / 442.81 | 409.68 / 438.83 |
| 四种 RAW 合计 | 420.20 / 470.74 | 407.49 / 453.18 |

**沿用旧口径，等胶片带布局稳定后再采样，混合 RAW 逐对差值的中位数仍多约 25 ms。** 这个口径会把清晰帧出现后到胶片带布局稳定的等待算进去。

### RAW 完整显影

各格式测 8 对，共 32 对。下表计时覆盖受监控的 LibRaw 完整解码和像素传输。另在 Windows 窗口启用「完整 RAW 解码」，四种样本均在图像属性中确认了完整显影及输出尺寸。

| 样本 | v0.8.0 P50 / P95（ms） | v0.8.1 P50 / P95（ms） | 进程树内存峰值（MiB） |
| --- | ---: | ---: | ---: |
| Nikon Z6 NEF | 3,344.0 / 3,375.0 | 3,359.5 / 3,375.0 | 310.0 → 310.0 |
| Canon EOS R CR3 | 3,961.0 / 4,000.0 | 3,985.0 / 4,032.0 | 384.7 → 384.7 |
| Sony a7 III ARW | 3,062.5 / 3,125.0 | 3,063.0 / 3,109.0 | 307.0 → 307.0 |
| iPhone 12 Pro DNG | 1,179.5 / 1,204.0 | 1,172.0 / 1,203.0 | 227.1 → 227.0 |

四种样本的显影中位数变动均在 24 ms 以内。内存列取显影测速进程树的私有提交内存采样峰值。另做的四种完整显影窗口检查中，v0.8.1 进程树私有提交内存的最高采样值为 522.5 MiB。

### 万张 JPEG、混合 RAW 与大 ZIP 回归

万张 JPEG 和混合 RAW 连翻各测 24 对，第二轮反转版本启动顺序。大 ZIP 含 241 张 JPEG，约 136.5 MiB，测 8 对。各轮预热样本不计入统计。长按翻图以目标图在原生窗口可见为准，JPEG 表内取正向翻图。

| 工作负载 | 指标 | v0.8.0 P50 / P95（ms） | v0.8.1 P50 / P95（ms） |
| --- | --- | ---: | ---: |
| 万张 JPEG | 首张清晰图 | 415.81 / 514.74 | 421.00 / 511.91 |
| 万张 JPEG | 长按到目标可见 | 49.25 / 95.00 | 48.88 / 95.37 |
| 混合 RAW | 长按到目标可见 | 89.73 / 161.01 | 90.66 / 144.21 |
| 大 ZIP | 首张清晰图 | 1,285.73 / 1,318.36 | 1,290.62 / 1,316.61 |
| 大 ZIP | 长按到目标可见 | 336.92 / 362.18 | 337.21 / 341.83 |
| 大 ZIP | 松开方向键到目标清晰 | 516.62 / 524.85 | 532.72 / 541.74 |

| 进程树内存峰值（MiB） | v0.8.0 | v0.8.1 |
| --- | ---: | ---: |
| 万张 JPEG | 437.1 | 437.3 |
| 混合 RAW | 431.6 | 430.3 |
| 大 ZIP | 591.1 | 561.6 |

内存为 RustView 与子进程私有提交内存的最高采样值。ZIP 连翻采样中未出现空白或串图。Windows x64 单测通过 302 项，默认跳过 3 项可选测试。本机双击 JPG、PNG 和右键打开 ZIP、RAR、7z 的检查均启动了 v0.8.1。图库数据库在升级前后校验一致。

### 完整离线版

完整解压 Windows x64 ZIP，双击 `RustView.exe` 或 `Start-RustView.cmd` 即可运行。包内提供图像和压缩包引擎、运行库、Tesseract、简体中文和英文 OCR 模型，首次启动即可离线使用。升级到新目录时，先关闭 RustView，再把旧目录中的 `RustView-data` 复制到新程序旁。 [完整离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.8.1/RustView-0.8.1-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.8.1/RustView-0.8.1-win-x64-offline.zip.sha256)

## English

### Draw the first sharp RAW frame sooner

When opening a RAW file directly, RustView draws the first sharp frame before initializing fonts and the catalog. RustView reads decoded pixels back in chunks to reduce redundant writes to the pixel buffer. Startup tracing records the sharp frame and interface initialization separately.

### Drag a divider between A and B

Select at least two images in Manage and press **Ctrl+Alt+R**. Select the reference and choose **固定参考图 A** (Pin reference A), then **分割对照** (Split comparison). Drag the divider to reveal more of either image on the shared canvas. A and B share zoom and pan. Use the arrow keys to change B. Hold **按住看参考图 A** (Hold to view reference A) to see the entire reference and release to return to the split view.

Native Windows tests used a PSD and transparent PNGs with identical dimensions. At 1:1, the marker stayed aligned while dragging the divider, holding to view A, and rapidly switching B. Transparent areas showed the expected checkerboard.

### Default RAW browsing

Default browsing prefers the camera's embedded preview when it is usable. The builds alternated in fresh processes on the same Windows PC with the same offline runtime files. Each of the four RAW samples had six measured pairs, for 24 pairs in total after excluding warm-up. Timing ran from launch to the first sharp frame captured in the native window. The sampler recognized sharp frames both before and after the filmstrip appeared. P50 is the median and P95 is the 95th percentile.

| Sample | v0.8.0 P50 / P95 (ms) | v0.8.1 P50 / P95 (ms) |
| --- | ---: | ---: |
| Nikon Z6 NEF | 409.47 / 457.48 | 403.39 / 406.74 |
| Canon EOS R CR3 | 457.21 / 472.15 | 448.95 / 455.70 |
| Sony a7 III ARW | 336.93 / 373.14 | 338.41 / 342.04 |
| iPhone 12 Pro DNG | 433.49 / 442.81 | 409.68 / 438.83 |
| All four RAW formats | 420.20 / 470.74 | 407.49 / 453.18 |

**With the earlier sampler, which waits for the filmstrip layout to settle, mixed RAW still took about 25 ms longer at the paired median.** That measurement includes the time between the first sharp frame and the stable filmstrip layout.

### Full RAW development

Each format had eight measured pairs, for 32 pairs in total. The table measures the supervised LibRaw full decode and pixel transfer. In separate Windows checks, enabling **完整 RAW 解码** (Full RAW decoding) confirmed full development and the output dimensions in the image properties for all four samples.

| Sample | v0.8.0 P50 / P95 (ms) | v0.8.1 P50 / P95 (ms) | Peak process-tree memory (MiB) |
| --- | ---: | ---: | ---: |
| Nikon Z6 NEF | 3,344.0 / 3,375.0 | 3,359.5 / 3,375.0 | 310.0 → 310.0 |
| Canon EOS R CR3 | 3,961.0 / 4,000.0 | 3,985.0 / 4,032.0 | 384.7 → 384.7 |
| Sony a7 III ARW | 3,062.5 / 3,125.0 | 3,063.0 / 3,109.0 | 307.0 → 307.0 |
| iPhone 12 Pro DNG | 1,179.5 / 1,204.0 | 1,172.0 / 1,203.0 | 227.1 → 227.0 |

The median full-development time changed by at most 24 ms across the four formats. The memory column shows the highest sampled private committed memory for the decoding process tree. In the separate full-development window checks, the highest process-tree sample for v0.8.1 was 522.5 MiB.

### 10,000 JPEGs, mixed RAW, and a large ZIP

The 10,000-JPEG and mixed RAW browsing tests had 24 pairs each, with the launch order reversed in the second round. The roughly 136.5 MiB ZIP contained 241 JPEGs and had eight pairs. Warm-up samples were excluded. Held-arrow timings measure when the target appears in the native window. The JPEG row uses forward navigation.

| Workload | Metric | v0.8.0 P50 / P95 (ms) | v0.8.1 P50 / P95 (ms) |
| --- | --- | ---: | ---: |
| 10,000 JPEGs | First sharp image | 415.81 / 514.74 | 421.00 / 511.91 |
| 10,000 JPEGs | Held arrow to visible target | 49.25 / 95.00 | 48.88 / 95.37 |
| Mixed RAW | Held arrow to visible target | 89.73 / 161.01 | 90.66 / 144.21 |
| Large ZIP | First sharp image | 1,285.73 / 1,318.36 | 1,290.62 / 1,316.61 |
| Large ZIP | Held arrow to visible target | 336.92 / 362.18 | 337.21 / 341.83 |
| Large ZIP | Key release to sharp target | 516.62 / 524.85 | 532.72 / 541.74 |

| Peak process-tree memory (MiB) | v0.8.0 | v0.8.1 |
| --- | ---: | ---: |
| 10,000 JPEGs | 437.1 | 437.3 |
| Mixed RAW | 431.6 | 430.3 |
| Large ZIP | 591.1 | 561.6 |

Memory figures are the highest observed samples of private committed memory for RustView and its child processes. The ZIP samples showed zero blank or wrong-image frames. The Windows x64 suite passed 302 tests with three optional tests skipped. Double-clicking JPG and PNG files or opening ZIP, RAR, and 7z files from their context menus launched v0.8.1 in local checks. The catalog checksum matched before and after the upgrade.

### Complete offline edition

Extract the entire Windows x64 ZIP and double-click `RustView.exe` or `Start-RustView.cmd`. The bundle includes the image and archive engines, runtime libraries, Tesseract, and Simplified Chinese and English OCR models. It works offline from the first launch. When upgrading to a new folder, close RustView and copy `RustView-data` from the old folder alongside the new executable. [Complete offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.8.1/RustView-0.8.1-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.8.1/RustView-0.8.1-win-x64-offline.zip.sha256)
