# RustView v0.7.9 · 压缩包拖动跳页

[English](#english)

## 简体中文

### 点击底部页码，拖到想看的那一张

打开多页 ZIP／RAR／7z 后，点击胶片带下方的页码计数，例如 `1 / 241`。拖动「压缩包跳页」中的滑块，面板会显示目标页码、文件名和已有的缓存缩略图。拖动期间主画面保留当前图片。松手提交选中的一页并按需读取，随后可用左右键沿包内浏览顺序继续翻图。加密包按提示输入密码即可使用。

原生窗口实测中，三个压缩包在拖动期间均未启动提取，松手后各提取目标页一次。测试也覆盖 Esc 取消拖动，以及目标读取途中点击「取消」和关闭窗口。续看位置保留在最后实际显示的图片，观测到的进程均退出，临时目录为空。[验证记录](https://github.com/walker1911/RustView/blob/v0.7.9/docs/VALIDATION.md#v079-windows-x64)

| 从第 1 页拖到未缓存的第 241 页 | 松手到目标可见 | 松手到清晰 |
| --- | ---: | ---: |
| 大 ZIP | 462 ms | 629 ms |
| 加密固实 RAR | 1,116 ms | 1,299 ms |
| 加密固实 7z | 3,388 ms | 3,571 ms |

这组跳页时间来自 v0.7.9 各包的一次观测。固实包跳到靠后的条目时，读取目标可能需要解压同一块中的前置数据。

### 和 v0.7.8 同机对照

两版在同一台 Windows 电脑上交替运行，使用同一套离线运行库。每次用新进程，首对预热不计入统计。大 ZIP、加密固实 RAR 和 7z 各 241 页、约 134–137 MiB，后两种为单固实块且启用了文件头加密。每个包测 6 对，万张 JPEG 和混合 RAW 各测 12 对。混合 RAW 涵盖尼康、佳能、索尼及苹果相机样本。时间以原生窗口的图像像素和目标标识为准。

大 ZIP 的首张清晰图从启动算起，加密包从提交密码算起，本地图片从启动算起。P50 为中位数，P95 为第 95 百分位，在这些样本量下容易受单次慢样本影响。

| 工作负载 | 指标 | v0.7.8 P50 / P95（ms） | v0.7.9 P50 / P95（ms） |
| --- | --- | ---: | ---: |
| 大 ZIP | 首张清晰图 | 1,303.2 / 1,344.0 | 1,318.3 / 1,351.1 |
| 大 ZIP | 长按到目标可见 | 341.4 / 379.9 | 346.6 / 358.9 |
| 大 ZIP | 松键到目标清晰 | 541.6 / 575.5 | 549.8 / 574.9 |
| 加密固实 RAR | 首张清晰图 | 850.9 / 900.4 | 859.0 / 900.6 |
| 加密固实 RAR | 长按到目标可见 | 657.5 / 673.3 | 657.2 / 660.3 |
| 加密固实 RAR | 松键到目标清晰 | 525.4 / 541.6 | 537.6 / 558.3 |
| 加密固实 7z | 首张清晰图 | 1,258.7 / 1,292.2 | 1,270.2 / 1,284.0 |
| 加密固实 7z | 长按到目标可见 | 1,796.8 / 1,825.4 | 1,808.2 / 1,821.8 |
| 加密固实 7z | 松键到目标清晰 | 560.1 / 575.6 | 541.4 / 550.4 |
| 万张 JPEG | 首图可见 | 611.0 / 645.7 | 608.2 / 728.5 |
| 万张 JPEG | 首张清晰图 | 626.3 / 661.5 | 623.4 / 744.0 |
| 万张 JPEG | 长按到目标可见 | 20.0 / 36.8 | 20.7 / 29.7 |
| 万张 JPEG | 松键到目标清晰 | 31.9 / 245.3 | 32.6 / 244.8 |
| 混合 RAW | 首图可见 | 593.7 / 848.1 | 607.2 / 702.1 |
| 混合 RAW | 首张清晰图 | 612.2 / 865.1 | 621.0 / 714.2 |
| 混合 RAW | 长按到目标可见 | 11.3 / 90.5 | 11.7 / 99.1 |
| 混合 RAW | 松键到目标清晰 | 303.8 / 394.5 | 294.8 / 404.1 |

五组的连翻目标可见中位数接近。采样峰值取 RustView 及其子进程的私有提交内存，统计范围在进程内存，GPU 内存和系统文件缓存另计。

| 进程树采样峰值（MiB） | v0.7.8 | v0.7.9 |
| --- | ---: | ---: |
| 大 ZIP | 590.8 | 560.8 |
| 加密固实 RAR | 629.2 | 598.1 |
| 加密固实 7z | 615.0 | 627.6 |
| 万张 JPEG | 473.8 | 435.0 |
| 混合 RAW | 447.4 | 430.9 |

### JPEG 首图排查

启动跟踪分别记录了进程创建、渲染初始化、首帧绘制、窗口显示和图像像素捕获。测试脚本原有的同步窗口置顶调用可额外阻塞约 190–289 毫秒，修正后使用非阻塞采样。控制试验测到的是采样开销，实际启动中的偶发慢样本还需结合各阶段时间判断。

用同一份 1,280 × 800 JPEG 直接启动两版，在本地目录及 OneDrive 桌面各测 8 对。OneDrive 上的样本已在本地可用。以下计时包括进程启动到画面第一次出现。

| JPEG 首图可见 | v0.7.8 P50 / P95（ms） | v0.7.9 P50 / P95（ms） |
| --- | ---: | ---: |
| 本地目录 | 383.4 / 404.3 | 368.0 / 586.3 |
| OneDrive 桌面 | 385.1 / 473.3 | 372.9 / 417.9 |

**JPEG 首图仍有波动。** 新版一次本地测试耗时 586 毫秒，其中渲染初始化约 350 毫秒，高于两版约 242–247 毫秒的中位数。万张 JPEG 的首张清晰图中位数接近，P95 从 661.5 增到 744.0 毫秒。排查未发现稳定复现的应用启动瓶颈，这次 JPEG 相关修正落在测量脚本。

### 完整离线版

完整解压 Windows x64 ZIP 后双击 `RustView.exe` 或 `Start-RustView.cmd`。图像和压缩包引擎、运行库、Tesseract、简体中文和英文 OCR 模型均随包提供，首次启动即可离线使用。升级到新目录时，先关闭 RustView，再把旧目录中的 `RustView-data` 复制到新程序旁。[完整离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.9/RustView-0.7.9-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.7.9/RustView-0.7.9-win-x64-offline.zip.sha256)

## English

### Click the page counter and drag to the image you want

Open a ZIP, RAR, or 7z archive with multiple images and click the counter below the filmstrip, such as `1 / 241`. Drag the slider in **压缩包跳页** (Jump within archive) to see the target page number, filename, and thumbnail when cached. The main view keeps the current picture while you drag. Release to load the selected page on demand, then continue through the archive with the left and right arrow keys. Encrypted archives work after you enter the password.

In native-window tests, dragging started zero extractions in all three archives. Releasing the slider extracted the selected page once per archive. Tests covered pressing Esc while dragging, choosing Cancel during the target read, and closing the window mid-read. The resume position stayed on the last image actually shown. All observed processes exited and the temporary directories were empty. [Validation record](https://github.com/walker1911/RustView/blob/v0.7.9/docs/VALIDATION.md#v079-windows-x64)

| Drag from page 1 to uncached page 241 | Release to visible target | Release to sharp target |
| --- | ---: | ---: |
| Large ZIP | 462 ms | 629 ms |
| Encrypted solid RAR | 1,116 ms | 1,299 ms |
| Encrypted solid 7z | 3,388 ms | 3,571 ms |

The jump timings are single v0.7.9 observations for each archive. Reaching a later entry in a solid archive can require decompressing earlier data in the same block.

### Measured against v0.7.8

The builds alternated on the same Windows PC with the same offline runtime. Each trial used a fresh process and each group excluded its first warm-up pair. The large ZIP, encrypted solid RAR, and encrypted solid 7z contained 241 pages each and were about 134–137 MiB each. The RAR and 7z used single solid blocks and encrypted headers. There were six pairs per archive and 12 pairs each for the 10,000-JPEG and mixed RAW workloads. Mixed RAW included Nikon, Canon, Sony, and Apple camera samples. Timings use pixels in the native window and target identity.

Time to the first sharp image starts at launch for ZIP and local images and at password submission for the encrypted archives. P50 is the median and P95 is the 95th percentile. At these sample sizes, one slow run can move P95 substantially.

| Workload | Metric | v0.7.8 P50 / P95 (ms) | v0.7.9 P50 / P95 (ms) |
| --- | --- | ---: | ---: |
| Large ZIP | First sharp image | 1,303.2 / 1,344.0 | 1,318.3 / 1,351.1 |
| Large ZIP | Held arrow to visible target | 341.4 / 379.9 | 346.6 / 358.9 |
| Large ZIP | Key release to sharp target | 541.6 / 575.5 | 549.8 / 574.9 |
| Encrypted solid RAR | First sharp image | 850.9 / 900.4 | 859.0 / 900.6 |
| Encrypted solid RAR | Held arrow to visible target | 657.5 / 673.3 | 657.2 / 660.3 |
| Encrypted solid RAR | Key release to sharp target | 525.4 / 541.6 | 537.6 / 558.3 |
| Encrypted solid 7z | First sharp image | 1,258.7 / 1,292.2 | 1,270.2 / 1,284.0 |
| Encrypted solid 7z | Held arrow to visible target | 1,796.8 / 1,825.4 | 1,808.2 / 1,821.8 |
| Encrypted solid 7z | Key release to sharp target | 560.1 / 575.6 | 541.4 / 550.4 |
| 10,000 JPEGs | First visible image | 611.0 / 645.7 | 608.2 / 728.5 |
| 10,000 JPEGs | First sharp image | 626.3 / 661.5 | 623.4 / 744.0 |
| 10,000 JPEGs | Held arrow to visible target | 20.0 / 36.8 | 20.7 / 29.7 |
| 10,000 JPEGs | Key release to sharp target | 31.9 / 245.3 | 32.6 / 244.8 |
| Mixed RAW | First visible image | 593.7 / 848.1 | 607.2 / 702.1 |
| Mixed RAW | First sharp image | 612.2 / 865.1 | 621.0 / 714.2 |
| Mixed RAW | Held arrow to visible target | 11.3 / 90.5 | 11.7 / 99.1 |
| Mixed RAW | Key release to sharp target | 303.8 / 394.5 | 294.8 / 404.1 |

Median time to a visible target during held-arrow browsing was similar across the five workloads. The sampled memory peaks cover private committed memory for RustView and its child processes. GPU memory and the system file cache are outside these measurements.

| Sampled process-tree peak (MiB) | v0.7.8 | v0.7.9 |
| --- | ---: | ---: |
| Large ZIP | 590.8 | 560.8 |
| Encrypted solid RAR | 629.2 | 598.1 |
| Encrypted solid 7z | 615.0 | 627.6 |
| 10,000 JPEGs | 473.8 | 435.0 |
| Mixed RAW | 447.4 | 430.9 |

### JPEG startup investigation

Startup tracing followed process creation, renderer initialization, first draw, window visibility, and image-pixel capture. A synchronous call to put the window on top in the earlier test harness could block for about 190–289 ms. Corrected capture runs use nonblocking sampling. The control measured overhead in the test harness. Occasional slow launches still need to be traced to the stage where they occur.

Both builds were launched directly with the same 1,280 × 800 JPEG, with eight pairs from a local folder and eight from the OneDrive Desktop. The OneDrive copy was already available locally. The table measures launch through the first visible image.

| JPEG first visible image | v0.7.8 P50 / P95 (ms) | v0.7.9 P50 / P95 (ms) |
| --- | ---: | ---: |
| Local folder | 383.4 / 404.3 | 368.0 / 586.3 |
| OneDrive Desktop | 385.1 / 473.3 | 372.9 / 417.9 |

**JPEG startup times still vary.** One v0.7.9 local run took 586 ms. Its renderer initialization took about 350 ms, compared with medians of about 242–247 ms across the builds and paths. The median time to the first sharp image in the 10,000-JPEG workload was similar, with P95 rising from 661.5 to 744.0 ms. Testing did not establish a consistently reproducible startup bottleneck in RustView. The investigation corrected the test harness's capture method.

### Complete offline edition

Extract the entire Windows x64 ZIP and double-click `RustView.exe` or `Start-RustView.cmd`. The bundle includes the image and archive engines, runtime libraries, Tesseract, and Simplified Chinese and English OCR models. It works offline from the first launch. When upgrading to a new folder, close RustView and copy `RustView-data` from the old folder alongside the new executable. [Complete offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.9/RustView-0.7.9-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.7.9/RustView-0.7.9-win-x64-offline.zip.sha256)
