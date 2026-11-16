# RustView v0.7.8 · ZIP 预览、RAW 内存与常用位置

[English](#english)

## 简体中文

- **常用位置。** 在「文件 → 最近打开…」中，点「添加文件夹…」或「添加压缩包…」，也可以从最近记录直接置顶。常用位置保存在本地目录库，重启后可继续使用。点文件夹或 ZIP／RAR／7z 的名称可以续看，点「从头打开」可以从第一张开始。重新打开加密包时仍需输入密码。
- **更快显示正在预读的 ZIP 图片。** 长按方向键选中正在预读的 ZIP 内 JPEG 时，符合条件的目标图可以更早显示自己的低清预览，清晰图复用这次提取的数据。继续翻页时，界面保留最近已经显示的画面，直到新目标可见。
- **减少 RAW 解码的临时内存。** 部分 RAW 内嵌 JPEG 的像素转换复用缓冲区，完整显影则更早释放 LibRaw 的临时数据。尼康 Z6、佳能 EOS R、索尼 a7 III、iPhone 12 Pro 的预览和完整解码像素均与 v0.7.7 一致。

### 与 v0.7.7 同机对照

同一台 Windows 电脑交替运行两版，使用同一套离线运行库。每次启动独立进程，两版共享预热后的系统缓存。普通 ZIP 和预读转前台各 10 对，本地万张 JPEG 和混合 RAW 各 12 对，每组排除首对预热。ZIP 样本含六张 5,040 × 3,360 合成 JPEG。预读转前台测试在上一张已清晰后再给预读 45 毫秒，然后用方向键选中预读目标。计时以窗口像素和目标文件名为准。

| 工作负载 | 指标 | v0.7.7 P50 / P95（ms） | v0.7.8 P50 / P95（ms） |
| --- | --- | ---: | ---: |
| 普通 ZIP | 启动到首张清晰图 | 1,338 / 1,452 | 1,344 / 1,392 |
| 普通 ZIP | 长按到目标图可见 | 333 / 361 | 343 / 362 |
| 普通 ZIP | 松键到目标图清晰 | 538 / 583 | 524 / 544 |
| ZIP 预读转前台 | 选中目标到低清可见 | 330 / 338 | 191 / 230 |
| ZIP 预读转前台 | 选中目标到清晰 | 525 / 547 | 521 / 563 |
| 万张 JPEG | 首图可见 | 672 / 875 | 701 / 910 |
| 万张 JPEG | 首张清晰图 | 687 / 883 | 716 / 926 |
| 万张 JPEG | 长按到目标图可见 | 21.2 / 36.9 | 16.7 / 45.5 |
| 万张 JPEG | 松键到目标图清晰 | 28.6 / 245.8 | 36.1 / 245.2 |
| 混合 RAW | 首图可见 | 725 / 784 | 660 / 815 |
| 混合 RAW | 首张清晰图 | 733 / 798 | 673 / 832 |
| 混合 RAW | 长按到目标图可见 | 11.4 / 100.4 | 11.4 / 99.6 |
| 混合 RAW | 松键到目标图清晰 | 298.6 / 402.1 | 306.4 / 393.6 |

P50 为中位数，P95 为第 95 百分位。普通 ZIP 的长按预览中位数增加约 10 毫秒，P95 接近，松键后清晰图提前约 14 毫秒。预读转前台时，低清中位数减少约 138 毫秒，清晰图的中位数接近，P95 增加约 17 毫秒。v0.7.7 曾出现的 793 毫秒 ZIP 慢帧在本轮未复现。

**JPEG 首图仍有波动。** 万张 JPEG 的首张清晰图中位数增加约 28 毫秒，P95 增加约 42 毫秒。另做 8 对启动跟踪，首图可见中位数为 709 → 710 毫秒，渲染初始化中位数约为 306 毫秒，JPEG 解码工作进程中位数为 218 → 217 毫秒。端到端首图耗时仍有波动。

| 进程树采样峰值（MiB） | v0.7.7 | v0.7.8 |
| --- | ---: | ---: |
| 普通 ZIP | 591.1 | 590.9 |
| ZIP 预读转前台 | 700.4 | 701.6 |
| 万张 JPEG | 474.6 | 439.4 |
| 混合 RAW | 428.8 | 431.8 |

此处约每 90 毫秒对 RustView 及其子进程的私有提交内存采样，GPU 和系统文件缓存不在统计内。混合图库的整棵进程树峰值接近。另用最终版运行独立 RAW 解码进程，对照两版的 RGBA 输出摘要，四份样本的预览和完整解码均匹配。内存峰值如下，单位为 MiB。

| RAW 样本 | 内嵌预览 v0.7.7 → v0.7.8 | 完整解码 v0.7.7 → v0.7.8 |
| --- | ---: | ---: |
| 尼康 Z6 NEF | 169.2 → 106.3 | 401.8 → 308.1 |
| 佳能 EOS R CR3 | 208.2 → 127.7 | 498.8 → 382.8 |
| 索尼 a7 III ARW | 17.3 → 13.4 | 397.8 → 305.1 |
| iPhone 12 Pro DNG | 95.8 → 95.8 | 271.8 → 225.2 |

Windows x64 单测通过 299 项，默认跳过 3 项可选测试。实际窗口验证了本地文件夹、ZIP、加密 RAR 和加密 7z 的置顶、重启后续看、从头打开和取消置顶。ZIP 的长按翻图采样未见白屏或串图。反向翻图、取消读取和关窗后，测试均确认所观测的工作进程退出，临时目录为空。[验证记录](https://github.com/walker1911/RustView/blob/v0.7.8/docs/VALIDATION.md)

### 完整离线版

完整解压 Windows x64 ZIP 后双击 `RustView.exe` 或 `Start-RustView.cmd`。图像和压缩包引擎、运行库、Tesseract 及简体中文和英文 OCR 模型均随包提供，首次启动即可离线使用。升级到新目录时，先关闭 RustView，再把旧目录中的 `RustView-data` 复制到新程序旁。[完整离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.8/RustView-0.7.8-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.7.8/RustView-0.7.8-win-x64-offline.zip.sha256)

## English

- **Pinned locations.** Open **文件 → 最近打开…** (File → Recent) and use **添加文件夹…** (Add folder), **添加压缩包…** (Add archive), or **置顶** (Pin) on a recent item. Pinned locations persist across launches. Select a folder or a ZIP, RAR, or 7z archive to resume, or choose **从头打开** (Open from start) to begin at the first picture. Encrypted archives still ask for their password when reopened.
- **Earlier previews for ZIP images already being prefetched.** When a held arrow key selects an eligible JPEG already being prefetched, RustView can show that image's own low-resolution preview sooner. The sharp image uses the same extracted data. If you keep browsing, the last displayed image stays visible while the next target loads.
- **Lower temporary memory during RAW decode.** Pixel conversion reuses a buffer for eligible embedded JPEGs, and full decode releases temporary LibRaw data earlier. The preview and full-decode pixels matched v0.7.7 for Nikon Z6, Canon EOS R, Sony a7 III, and iPhone 12 Pro samples.

### Measured against v0.7.7

Both builds alternated on the same Windows PC with the same offline runtime. Each trial started a fresh process and shared a warm system cache. The ordinary ZIP and prefetch-selection tests ran 10 pairs each. The local 10,000-JPEG and mixed RAW tests ran 12 pairs each. Every group excluded its first warm-up pair. The ZIP contained six synthetic 5,040 × 3,360 JPEGs. The prefetch-selection test gave the next image 45 ms to prefetch after the preceding image was sharp, then selected it with an arrow key. Timings use window pixels and target filenames.

| Workload | Metric | v0.7.7 P50 / P95 (ms) | v0.7.8 P50 / P95 (ms) |
| --- | --- | ---: | ---: |
| Ordinary ZIP | Launch to first sharp image | 1,338 / 1,452 | 1,344 / 1,392 |
| Ordinary ZIP | Held arrow to visible target | 333 / 361 | 343 / 362 |
| Ordinary ZIP | Key release to sharp target | 538 / 583 | 524 / 544 |
| Prefetched ZIP target | Selection to visible preview | 330 / 338 | 191 / 230 |
| Prefetched ZIP target | Selection to sharp image | 525 / 547 | 521 / 563 |
| 10,000 JPEGs | First visible image | 672 / 875 | 701 / 910 |
| 10,000 JPEGs | First sharp image | 687 / 883 | 716 / 926 |
| 10,000 JPEGs | Held arrow to visible target | 21.2 / 36.9 | 16.7 / 45.5 |
| 10,000 JPEGs | Key release to sharp target | 28.6 / 245.8 | 36.1 / 245.2 |
| Mixed RAW | First visible image | 725 / 784 | 660 / 815 |
| Mixed RAW | First sharp image | 733 / 798 | 673 / 832 |
| Mixed RAW | Held arrow to visible target | 11.4 / 100.4 | 11.4 / 99.6 |
| Mixed RAW | Key release to sharp target | 298.6 / 402.1 | 306.4 / 393.6 |

P50 is the median and P95 is the 95th percentile. In ordinary ZIP browsing, the median preview took about 10 ms longer, P95 was similar, and the sharp image after key release arrived about 14 ms sooner. In the prefetch-selection test, the median preview arrived about 138 ms sooner. Median time to the sharp image was similar, with P95 about 17 ms higher. The earlier v0.7.7 ZIP outlier of 793 ms did not recur in these tests.

**JPEG startup times still vary.** In the 10,000-JPEG test, the median time to the first sharp image rose about 28 ms and P95 rose about 42 ms. In eight additional paired startup traces, the median time to the first visible image was 709 → 710 ms. Renderer initialization took about 306 ms in both builds and median JPEG worker decode time was 218 → 217 ms. End-to-end startup times still varied.

| Sampled process-tree peak (MiB) | v0.7.7 | v0.7.8 |
| --- | ---: | ---: |
| Ordinary ZIP | 591.1 | 590.9 |
| Prefetched ZIP target | 700.4 | 701.6 |
| 10,000 JPEGs | 474.6 | 439.4 |
| Mixed RAW | 428.8 | 431.8 |

Private committed memory was sampled across RustView and its child processes roughly every 90 ms, excluding GPU memory and the system file cache. Process-tree peaks were similar in the mixed RAW workload. The final build also ran standalone RAW decode checks. All four samples matched v0.7.7's RGBA output hashes in both preview and full-decode modes. Worker peak memory for these samples is in MiB.

| RAW sample | Embedded preview v0.7.7 → v0.7.8 | Full decode v0.7.7 → v0.7.8 |
| --- | ---: | ---: |
| Nikon Z6 NEF | 169.2 → 106.3 | 401.8 → 308.1 |
| Canon EOS R CR3 | 208.2 → 127.7 | 498.8 → 382.8 |
| Sony a7 III ARW | 17.3 → 13.4 | 397.8 → 305.1 |
| iPhone 12 Pro DNG | 95.8 → 95.8 | 271.8 → 225.2 |

The Windows x64 suite passed 299 unit tests, with three optional tests skipped by default. Native-window tests verified pinning, resuming after restart, opening from start, and unpinning for a local folder, ZIP, encrypted RAR, and encrypted 7z. Sampling during held-arrow ZIP navigation found no blank or wrong-image frames. Reversing direction, cancelling a read, and closing the viewer each ended with all observed workers stopped and the temporary directory empty. [Validation record](https://github.com/walker1911/RustView/blob/v0.7.8/docs/VALIDATION.md)

### Complete offline version

Extract the entire Windows x64 ZIP and double-click `RustView.exe` or `Start-RustView.cmd`. The bundle includes the image and archive engines, runtime libraries, Tesseract, and Simplified Chinese and English OCR models. It works offline from the first launch. When upgrading to a new folder, close RustView and copy `RustView-data` from the old folder alongside the new executable. [Complete offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.8/RustView-0.7.8-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.7.8/RustView-0.7.8-win-x64-offline.zip.sha256)
