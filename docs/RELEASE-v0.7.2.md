# RustView v0.7.2 · 本地文件夹跳图 / Jump to images in local folders

## 中文

- **按文件名或序号定位。** 在本地文件夹单图查看中，等首次目录扫描完成，移到窗口顶边并点击「跳页…」。支持不区分大小写的部分文件名和从 1 开始的浏览序号。有唯一最佳匹配时可直接回车，有多个同等匹配时在列表里选一张。跳转后，左右键沿同一顺序继续浏览。
- **目录变化时同步结果。** 本地目录自动刷新会更新弹窗的匹配结果和序号。目录恰好变化时，程序等你确认新结果。打开 ZIP、RAR、7z 时仍可用完整包内路径区分重名图片。

### Windows 实测

在万张 JPEG 目录中，按序号跳到第 6,800 张，再按右键到第 6,801 张。另按文件名跳到第 2,100 张，再按左键回到第 2,099 张。也验证了搜索弹窗打开时文件改名、混合 RAW 目录跳图，以及压缩包内按完整路径跳转。上述测试关闭后未观察到残留工作进程或任务临时文件。277 项单元测试通过，3 项跳过。

v0.7.1 和 v0.7.2 在同一 Windows 电脑交替运行，使用预热过的系统文件缓存，检查文件标题和桌面画面。表中为中位数。单次按键覆盖新配置和复用配置，每种 6 次。首图清晰时只按一次左键或右键，两版都成功 12／12 次。长按测试每版各对万张 JPEG 目录和相机 RAW 运行 8 次。

| Windows 桌面测量 | v0.7.1 | v0.7.2 |
|---|---:|---:|
| 启动到首图清晰，单次按键测试，新配置／复用配置 | 747／745 ms | 753／729 ms |
| 首次按键到邻图可见，新配置／复用配置 | 150／154 ms | 150／150 ms |
| 首次按键到邻图清晰，新配置／复用配置 | 300／304 ms | 304／312 ms |
| JPEG 长按到目标预览 | 74 ms | 78 ms |
| JPEG 松键到清晰图 | 77 ms | 73 ms |
| JPEG 点按回看缓存，到清晰图 | 74 ms | 70 ms |
| RAW 启动到首图清晰 | 762 ms | 746 ms |
| RAW 长按到目标预览 | 65 ms | 74 ms |
| RAW 松键到清晰图 | 348 ms | 361 ms |

RAW 的松键后清晰时间按两版各自取中位数为 **348 → 361 ms**，逐次配对差的中位数为 **−9 ms**。配对测试结果有快有慢。JPEG 及 RAW 长按测试的进程树私有提交峰值中位数分别为 **586 → 572 MiB** 和 **582 → 568 MiB**。单次按键测试的采样中位数在新配置为 **524 → 527 MiB**，复用配置为 **518 → 541 MiB**。每约 90 ms 采样，统计应用和工作进程的私有提交内存。首键和长按测试均未采到空白画面。

完整解压 Windows x64 离线 ZIP，再双击 `RustView.exe` 或 `Start-RustView.cmd`。随包提供 7-Zip、LibRaw、ImageMagick、ExifTool、VC++ 运行库、Tesseract 及其简体中文和英文模型，首次启动即可离线看图和识字。迁移目录时，关闭 RustView 后复制旧程序旁的 `RustView-data`，可保留设置、快捷键、目录库和处理记录。[离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.2/RustView-0.7.2-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.7.2/RustView-0.7.2-win-x64-offline.zip.sha256)

## English

- **Find a photo by name or number.** In a local folder's single-image view, wait for the initial scan, move the pointer to the top edge, and click **跳页…** (Jump to image). Search by case-insensitive partial filename or by number starting at 1 in the current browsing order. Press Enter for a unique best match or choose an entry when matches tie. Left and Right continue in the same order.
- **Keep search results current.** Automatic folder refresh updates the dialog's matches and numbers when local files change. If the order has just changed, RustView waits for confirmation of the updated result. In ZIP, RAR, and 7z archives, a full internal path still distinguishes files with the same name.

### Windows validation

In a folder of 10,000 JPEGs, the desktop test jumped to image 6,800 and moved right to 6,801. It also jumped to image 2,100 by name and moved left to 2,099. Other checks covered renaming a file with the dialog open, jumping in a mixed RAW folder, and selecting a full path inside an archive. No workers or task temporary files were found after these runs closed. 277 unit tests passed, with 3 ignored.

The same Windows PC alternated v0.7.1 and v0.7.2 with a warm filesystem cache, using window titles and desktop pixels to verify results. The table shows medians. The single-tap test ran six times each with a fresh application profile and a reused profile. Both builds navigated correctly in all 12 runs when Left or Right was pressed once as soon as the first photo was clear. Held-arrow tests ran eight times per version for each of the 10,000-JPEG folder and the camera RAW set.

| Windows desktop measurement | v0.7.1 | v0.7.2 |
|---|---:|---:|
| Launch to first clear image in the single-tap test, fresh / reused profile | 747 / 745 ms | 753 / 729 ms |
| First tap to visible neighbor, fresh / reused profile | 150 / 154 ms | 150 / 150 ms |
| First tap to clear neighbor, fresh / reused profile | 300 / 304 ms | 304 / 312 ms |
| Held-arrow JPEG navigation to target preview | 74 ms | 78 ms |
| JPEG key release to clear image | 77 ms | 73 ms |
| Cached JPEG tap to clear image | 74 ms | 70 ms |
| RAW launch to first clear image | 762 ms | 746 ms |
| Held-arrow RAW navigation to target preview | 65 ms | 74 ms |
| RAW key release to clear image | 348 ms | 361 ms |

For RAW key release to clear image, the separate version medians were **348 → 361 ms** and the median of the paired differences was **−9 ms**. Individual pairs varied. The median of sampled peak process-tree private commit was **586 → 572 MiB** for held-arrow JPEG and **582 → 568 MiB** for RAW. The single-tap median peaks were **524 → 527 MiB** with a fresh profile and **518 → 541 MiB** with a reused profile. Sampling roughly every 90 ms covered application and worker private commit. No blank frames were sampled in either the single-tap or held-arrow tests.

Fully extract the Windows x64 offline ZIP and double-click `RustView.exe` or `Start-RustView.cmd`. The ZIP includes 7-Zip, LibRaw, ImageMagick, ExifTool, app-local VC++ libraries, and Tesseract with Simplified Chinese and English models. Viewing and OCR work offline from the first launch. When moving to a new folder, close RustView and copy `RustView-data` from beside the old executable to keep settings, shortcuts, the catalog, and processing records. [Offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.7.2/RustView-0.7.2-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.7.2/RustView-0.7.2-win-x64-offline.zip.sha256)

[离线版说明 / Offline guide](https://github.com/walker1911/RustView/blob/v0.7.2/OFFLINE.md) · [上个版本 / Previous release](https://github.com/walker1911/RustView/releases/tag/v0.7.1)
