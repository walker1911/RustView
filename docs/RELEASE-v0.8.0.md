# RustView v0.8.0 · 固定 A/B 对照

[English](#english)

## 简体中文

### 固定参考图，在同一画布比较细节

在「管理」模式选中至少两张图片，按 **Ctrl+Alt+R** 打开比较。选中要固定的图，再点「固定参考图 A」。此时画布显示对照图 B，左右键可在所选图片中切换 B。按住「按住看参考图 A」在原位置查看 A，松手回到 B。两张图共用缩放和平移，查看同一区域时只需调整一次。点「返回并排比较」可回到多图布局。

Windows 原生窗口实测使用带标记的 PSD 和透明 PNG。确认了在 1:1 下，按住查看 A、松手和快速切换 B 时，标记落在同一像素位置。另用尼康、佳能和索尼 RAW 验证了切换。PSD／透明 PNG 和 RAW 对照测试的进程树内存峰值分别为 452.8 和 560.2 MiB。关窗后，所观测的工作进程均已退出，测试临时目录为空。Windows x64 单测通过 301 项，默认跳过 3 项可选测试。

### 和 v0.7.9 同机实测

两版使用同一台 Windows 电脑和同一套离线运行库，每次用新进程。分两轮交替测试，两轮的版本启动顺序相反，每轮首对预热样本不计入统计。万张 JPEG 和混合 RAW 各测 24 对，大 ZIP 测 12 对。RAW 样本涵盖尼康、佳能、索尼及苹果相机，ZIP 含 241 张 JPEG，约 136.5 MiB。

首张清晰图从启动算起，翻图时间以原生窗口中目标图像可见为准。万张 JPEG 表内的长按数据取正向翻图。P50 为中位数，P95 为第 95 百分位。

| 工作负载 | 指标 | v0.7.9 P50 / P95（ms） | v0.8.0 P50 / P95（ms） |
| --- | --- | ---: | ---: |
| 万张 JPEG | 首张清晰图 | 521.1 / 672.0 | 526.8 / 573.5 |
| 万张 JPEG | 长按到目标可见 | 48.8 / 102.4 | 48.7 / 94.7 |
| 混合 RAW | 首张清晰图 | 512.6 / 561.7 | 533.6 / 619.8 |
| 混合 RAW | 长按到目标可见 | 90.5 / 177.7 | 90.1 / 145.0 |
| 大 ZIP | 首张清晰图 | 1,335.4 / 1,368.2 | 1,310.7 / 1,330.4 |
| 大 ZIP | 长按到目标可见 | 337.4 / 347.8 | 338.0 / 370.8 |

**混合 RAW 的首张清晰图在两轮都稍慢。** 两轮合计中位数增加约 21 毫秒，P95 增加约 58 毫秒。三组长按翻图的目标可见中位数接近，大 ZIP 的该项 P95 增加约 23 毫秒。ZIP 翻图采样中未出现空白或串图，测试关窗后未发现进程或临时文件残留。

| 进程树采样峰值（MiB） | v0.7.9 | v0.8.0 |
| --- | ---: | ---: |
| 万张 JPEG | 427.8 | 427.6 |
| 混合 RAW | 430.1 | 448.6 |
| 大 ZIP | 554.1 | 591.4 |

内存为 RustView 与子进程私有提交内存的最高采样值，GPU 内存和系统文件缓存另计。v0.8.0 的 ZIP 峰值在第一轮为 591.4 MiB，反转启动顺序后为 553.3 MiB。

### 完整离线版

完整解压 Windows x64 ZIP，双击 `RustView.exe` 或 `Start-RustView.cmd`。图像和压缩包引擎、运行库、Tesseract、简体中文和英文 OCR 模型均随包提供，首次启动即可离线使用。升级到新目录时，先关闭 RustView，再把旧目录中的 `RustView-data` 复制到新程序旁。[完整离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.8.0/RustView-0.8.0-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.8.0/RustView-0.8.0-win-x64-offline.zip.sha256)

## English

### Pin a reference and compare details on the same canvas

In Manage mode, select at least two images and press **Ctrl+Alt+R** to open Compare. Select the image you want to pin and click **固定参考图 A** (Pin reference A). The canvas shows image B, and the left and right arrows switch B through the selection. Hold **按住看参考图 A** (Hold to view reference A) to see A in place, then release to return to B. Both images share zoom and pan, so you can set the view once and inspect the same area. Click **返回并排比较** (Return to side-by-side comparison) to restore the multi-image layout.

Native Windows tests used a marked PSD and transparent PNG. At 1:1, the marker landed at the same pixel position when showing A, releasing the button, and quickly switching B. Further tests covered switching between Nikon, Canon, and Sony RAW images. The PSD and transparent PNG comparison peaked at 452.8 MiB, and the RAW comparison at 560.2 MiB across the process tree. All observed workers had exited when the window closed, and the test temporary directories were empty. On Windows x64, 301 unit tests passed and three optional tests were skipped by default.

### Measured against v0.7.9

Both builds ran on the same Windows PC with identical offline runtime files and a fresh process for every trial. The builds alternated across two rounds with opposite starting orders. Each round excluded its first warm-up pair. The 10,000-JPEG and mixed RAW workloads each had 24 pairs, and the large ZIP had 12. The RAW set included Nikon, Canon, Sony, and Apple camera samples. The ZIP contained 241 JPEGs and was about 136.5 MiB.

Time to the first sharp image starts at launch. Browsing timings use the appearance of the target image in the native window. The 10,000-JPEG held-arrow timings below are for forward navigation. P50 is the median and P95 is the 95th percentile.

| Workload | Metric | v0.7.9 P50 / P95 (ms) | v0.8.0 P50 / P95 (ms) |
| --- | --- | ---: | ---: |
| 10,000 JPEGs | First sharp image | 521.1 / 672.0 | 526.8 / 573.5 |
| 10,000 JPEGs | Held arrow to visible target | 48.8 / 102.4 | 48.7 / 94.7 |
| Mixed RAW | First sharp image | 512.6 / 561.7 | 533.6 / 619.8 |
| Mixed RAW | Held arrow to visible target | 90.5 / 177.7 | 90.1 / 145.0 |
| Large ZIP | First sharp image | 1,335.4 / 1,368.2 | 1,310.7 / 1,330.4 |
| Large ZIP | Held arrow to visible target | 337.4 / 347.8 | 338.0 / 370.8 |

**Mixed RAW reached its first sharp image slightly more slowly in both rounds.** Across both rounds, its median rose by about 21 ms and its P95 by about 58 ms. Median held-arrow times were close across the three workloads. For the large ZIP, the P95 for that metric rose by about 23 ms. The ZIP browsing samples showed zero blank frames or wrong-image frames, with no surviving workers or temporary files observed after closing.

| Sampled process-tree peak (MiB) | v0.7.9 | v0.8.0 |
| --- | ---: | ---: |
| 10,000 JPEGs | 427.8 | 427.6 |
| Mixed RAW | 430.1 | 448.6 |
| Large ZIP | 554.1 | 591.4 |

Memory figures are the highest observed samples of private committed memory for RustView and its child processes. GPU memory and the system file cache are outside these measurements. The v0.8.0 ZIP peak was 591.4 MiB in the first round and 553.3 MiB after reversing the order.

### Complete offline edition

Extract the entire Windows x64 ZIP and double-click `RustView.exe` or `Start-RustView.cmd`. The bundle includes the image and archive engines, runtime libraries, Tesseract, and Simplified Chinese and English OCR models. It works offline from the first launch. When upgrading to a new folder, close RustView and copy `RustView-data` from the old folder alongside the new executable. [Complete offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.8.0/RustView-0.8.0-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.8.0/RustView-0.8.0-win-x64-offline.zip.sha256)
