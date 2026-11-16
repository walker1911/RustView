# RustView v0.7.7 · 包内 JPEG 连续翻图 / Faster JPEG browsing inside archives

## 简体中文

在 ZIP、RAR 和 7z 中长按方向键，符合条件且尚未缓存的 JPEG 会优先显示目标图自己的低清预览。松开方向键后，RustView 复用同一份已提取的数据解码清晰图。已有可用清晰图时会直接显示。反向翻图、取消和关窗会停止不再需要的工作。

### 与 v0.7.6 对照实测

两版使用同一套离线运行库，在同一台 Windows 电脑上交替测试。三个压缩包使用相同的六张合成 JPEG，尺寸均为 5,040 × 3,360。ZIP 无密码，RAR 和 7z 为加密固实压缩，每种格式各测 8 对。本地万张 JPEG 和混合 RAW 各测 24 对，分两轮交换起始顺序。计时依据实际窗口像素和目标文件名，每轮预热样本不纳入统计。

表中时间均为毫秒，每格依次给出 P50（中位数）和 P95（第 95 百分位）。

| 压缩包 | 指标 | v0.7.6 P50 / P95 | v0.7.7 P50 / P95 |
| --- | --- | ---: | ---: |
| ZIP | 启动到首张清晰图 | 1,274 / 1,331 | 1,304 / 1,363 |
| ZIP | 长按到目标图首次可见 | 445 / 449 | **337** / 793 |
| ZIP | 松键到目标图清晰 | −88 / −24 | 550 / 592 |
| 加密固实 RAR | 提交密码到首张清晰图 | 826 / 859 | 837 / 867 |
| 加密固实 RAR | 长按到目标图首次可见 | 725 / 760 | **633** / 649 |
| 加密固实 RAR | 松键到目标图清晰 | −133 / −117 | 525 / 558 |
| 加密固实 7z | 提交密码到首张清晰图 | 1,218 / 1,242 | 1,230 / 1,267 |
| 加密固实 7z | 长按到目标图首次可见 | 1,818 / 1,832 | **1,705** / 1,729 |
| 加密固实 7z | 松键到目标图清晰 | −395 / −383 | 525 / 542 |

负数表示松键前已显示清晰图。新版把符合条件的完整解码留到松键后，这组大 JPEG 随后约需 0.53–0.55 秒补清晰。ZIP 的中位数改善，但有一次目标图等待约 793 毫秒。每组只有 8 对，P95 对单次慢样本敏感。

| 本地工作负载 | 指标 | v0.7.6 P50 / P95 | v0.7.7 P50 / P95 |
| --- | --- | ---: | ---: |
| 万张 JPEG | 首图首次可见 | 480 / 563 | 481 / 627 |
| 万张 JPEG | 首张清晰图 | 494 / 579 | 495 / 642 |
| 万张 JPEG | 长按到目标图首次可见 | 49 / 95 | 49 / 96 |
| 万张 JPEG | 松键到目标图清晰 | 149 / 253 | 144 / 236 |
| 混合 RAW | 首图首次可见 | 388 / 509 | 378 / 413 |
| 混合 RAW | 首张清晰图 | 498 / 590 | 507 / 576 |
| 混合 RAW | 长按到目标图首次可见 | 91 / 145 | 90 / 144 |
| 混合 RAW | 松键到目标图清晰 | 182 / 387 | 204 / 386 |

本地长按到目标图的配对中位差小于 1 毫秒。万张 JPEG 的首图 P95 上升。混合 RAW 首张清晰图的配对中位耗时增加约 11 毫秒，松键到清晰图增加约 8 毫秒。

| 进程树采样峰值（MiB） | v0.7.6 | v0.7.7 |
| --- | ---: | ---: |
| ZIP | 624.8 | 576.6 |
| 加密固实 RAR | 629.3 | 628.5 |
| 加密固实 7z | 784.1 | 661.4 |
| 万张 JPEG | 436.5 | 437.4 |
| 混合 RAW | 429.8 | 508.7 |

内存统计取 RustView 及其子进程的私有提交量峰值，约每 90 毫秒采样一次，GPU 和系统文件缓存另计。压缩包内长按翻图的窗口采样中未观察到串图或白屏。对三种压缩包分别验证取消、反向翻图和关窗，共 9 项，均通过且未发现残留进程或临时文件。

### 完整离线版

完整解压 Windows x64 ZIP 后，双击 `RustView.exe` 或 `Start-RustView.cmd`。运行库、图像和压缩包引擎、Tesseract 及简体中文和英文 OCR 模型均随包提供，首次启动即可离线使用。升级到新目录时，先关闭 RustView，再将旧目录中的 `RustView-data` 复制到新程序旁。[下载完整离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.7/RustView-0.7.7-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.7.7/RustView-0.7.7-win-x64-offline.zip.sha256)

## English

While you hold an arrow key in a ZIP, RAR, or 7z archive, eligible uncached JPEGs show a low-resolution preview of the target image. On release, RustView reuses the same extracted data to decode the full image. If the full image is already available, RustView displays it immediately. Reversing direction, cancelling, or closing the window stops work that is no longer needed.

### Measured against v0.7.6

Both builds used the same offline runtime on one Windows PC and alternated run order. All three archives contained the same six synthetic 5,040 × 3,360 JPEGs. The ZIP was unencrypted. The solid RAR and 7z archives were encrypted. There were eight pairs per archive format and 24 pairs each for a local directory of 10,000 JPEGs and a mixed RAW workload. The local tests used two runs with opposite starting order. Timings used native window pixels and target filenames, with warm-up pairs excluded.

All timings are in milliseconds. Each cell gives P50 (median) followed by P95 (95th percentile).

| Archive | Metric | v0.7.6 P50 / P95 | v0.7.7 P50 / P95 |
| --- | --- | ---: | ---: |
| ZIP | Launch to first sharp image | 1,274 / 1,331 | 1,304 / 1,363 |
| ZIP | Held navigation to visible target | 445 / 449 | **337** / 793 |
| ZIP | Key release to sharp target | −88 / −24 | 550 / 592 |
| Encrypted solid RAR | Password submission to first sharp image | 826 / 859 | 837 / 867 |
| Encrypted solid RAR | Held navigation to visible target | 725 / 760 | **633** / 649 |
| Encrypted solid RAR | Key release to sharp target | −133 / −117 | 525 / 558 |
| Encrypted solid 7z | Password submission to first sharp image | 1,218 / 1,242 | 1,230 / 1,267 |
| Encrypted solid 7z | Held navigation to visible target | 1,818 / 1,832 | **1,705** / 1,729 |
| Encrypted solid 7z | Key release to sharp target | −395 / −383 | 525 / 542 |

Negative times indicate that the sharp image appeared before key release. The new preview path defers the full decode until release, after which these large JPEGs took about 0.53–0.55 seconds to sharpen. The ZIP median improved, with one target taking about 793 ms. With eight pairs per format, P95 is sensitive to a single slow run.

| Local workload | Metric | v0.7.6 P50 / P95 | v0.7.7 P50 / P95 |
| --- | --- | ---: | ---: |
| 10,000 JPEGs | First visible image | 480 / 563 | 481 / 627 |
| 10,000 JPEGs | First sharp image | 494 / 579 | 495 / 642 |
| 10,000 JPEGs | Held navigation to visible target | 49 / 95 | 49 / 96 |
| 10,000 JPEGs | Key release to sharp target | 149 / 253 | 144 / 236 |
| Mixed RAW | First visible image | 388 / 509 | 378 / 413 |
| Mixed RAW | First sharp image | 498 / 590 | 507 / 576 |
| Mixed RAW | Held navigation to visible target | 91 / 145 | 90 / 144 |
| Mixed RAW | Key release to sharp target | 182 / 387 | 204 / 386 |

The median paired difference in held navigation to a visible target was under 1 ms for both local workloads. First-image P95 increased in the 10,000-JPEG workload. In mixed RAW, the median paired differences were about 11 ms longer to the first sharp image and 8 ms longer from key release to a sharp target.

| Sampled process-tree peak (MiB) | v0.7.6 | v0.7.7 |
| --- | ---: | ---: |
| ZIP | 624.8 | 576.6 |
| Encrypted solid RAR | 629.3 | 628.5 |
| Encrypted solid 7z | 784.1 | 661.4 |
| 10,000 JPEGs | 436.5 | 437.4 |
| Mixed RAW | 429.8 | 508.7 |

Memory figures are peaks of private committed memory for RustView and its child processes, sampled roughly every 90 ms. GPU memory and the system file cache are outside these measurements. Window sampling during held navigation in archives detected no wrong-image or blank frames. Cancellation, reversing direction, and closing the window each passed in all three archive formats. These nine checks left no observed child processes or temporary files.

### Complete offline version

Extract the entire Windows x64 ZIP and double-click `RustView.exe` or `Start-RustView.cmd`. The bundle includes the runtime libraries, image and archive engines, Tesseract, and Simplified Chinese and English OCR models. It works offline from the first launch. When upgrading to a new folder, close RustView and copy `RustView-data` from the old folder alongside the new executable to keep your data. [Download the complete offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.7/RustView-0.7.7-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.7.7/RustView-0.7.7-win-x64-offline.zip.sha256)
