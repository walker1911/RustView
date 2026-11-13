# RustView v0.6.5 / 更新说明

## 中文

- **按住方向键快速看 RAW。** 对符合条件的本地 RAW，长按左右键时先显示目标图的内嵌预览，松键后按当前 RAW 解码设置读取最终选中的一张。没有可用内嵌预览、带已保存编辑记录或位于压缩包内的 RAW 走常规读取流程。
- **从压缩包第一张重新看。** 在「文件 → 最近打开…」中，点击 ZIP、RAR 或 7z 条目可续看，也可点该条目下的「从头打开」。加密包仍需输入密码。成功显示首图后，新的浏览位置会写入本地目录库。

### 实测与验证

在同一台 Windows 电脑上，用尼康 Z6、佳能 EOS R、索尼 a7 III 和 iPhone 12 Pro RAW 样本对比 v0.6.4 与 v0.6.5。每版有八次长按翻图，按键重复间隔 65 ms，最后一次按键后继续按住约 140 ms。画面计时来自桌面像素。

| 指标 | v0.6.4 | v0.6.5 |
|---|---:|---:|
| 松键前看到目标自己的预览 | 4 / 8 次 | 8 / 8 次 |
| 末次按键到目标预览可见，中位数 | 166 ms | 62 ms |
| 松键到适应窗口下的清晰画面，中位数 | 353 ms | 478 ms |

独立预览工作进程测试中，尼康样本的采样内存峰值为 171 → 9 MiB，佳能样本为 210 → 10 MiB。数值为预览工作进程树的私有提交内存，包含测试探针。原生窗口也验证了反向切换、长按时关窗，以及索尼样本的完整 RAW 解码。

原生窗口使用加密 ZIP、RAR 和 7z 验证了「从头打开」的密码提示和首图。7z 还验证了大 PSD 续看、错误密码及取消后保留原位置，ZIP 和 RAR 则使用预先写入的续看位置测试从头打开。上述测试结束后没有残留进程或临时文件。通过 237 项单测和 87 项解压包回归检查。

下载 Windows x64 完整离线 ZIP，完整解压后双击 `RustView.exe` 或 `Start-RustView.cmd`。7-Zip、LibRaw、ImageMagick、ExifTool 和 VC++ 运行库随包提供，首次启动可离线。迁移到新目录时，可关闭 RustView 后复制旧程序旁的 `RustView-data`，保留设置、目录库和处理记录。[离线 ZIP](https://github.com/walker1911/RustView/releases/download/v0.6.5/RustView-0.6.5-win-x64-offline.zip) · [SHA-256 校验文件](https://github.com/walker1911/RustView/releases/download/v0.6.5/RustView-0.6.5-win-x64-offline.zip.sha256)

## English

- **Quick RAW previews while holding an arrow key.** When you hold Left or Right on eligible local RAW files, RustView shows the selected image's embedded preview first. When you release the key, it reads the final selection using the current RAW decode setting. RAWs without a usable embedded preview, with saved edits, or inside an archive follow the regular read path.
- **Open an archive from its first image.** In **文件 → 最近打开…** (File → Recent), select a ZIP, RAR, or 7z archive to resume, or choose **从头打开** (Open from start) below it. Encrypted archives still prompt for a password. Once the first image is shown, its position is saved in the local catalog.

### Measurements and validation

The same Windows PC ran v0.6.4 and v0.6.5 with Nikon Z6, Canon EOS R, Sony a7 III, and iPhone 12 Pro RAW samples. Each build ran eight holds with 65 ms key repeats and a roughly 140 ms hold after the final repeat. Timings come from desktop pixels.

| Measurement | v0.6.4 | v0.6.5 |
|---|---:|---:|
| Selected image preview visible before release | 4 / 8 | 8 / 8 |
| Final keydown to visible target preview, median | 166 ms | 62 ms |
| Key release to clear image at fit zoom, median | 353 ms | 478 ms |

In a separate preview worker test, sampled peak private commit fell from 171 to 9 MiB for the Nikon sample and from 210 to 10 MiB for the Canon sample. These figures cover the preview worker tree, including the test probe. Native-window checks also exercised direction reversal, closing while holding a key, and full RAW decoding for the Sony sample.

Native-window checks verified the password prompt and first image when opening encrypted ZIP, RAR, and 7z archives from the start. The 7z check also resumed at a large PSD, rejected a wrong password, and preserved the old position after cancellation. The ZIP and RAR checks used a prewritten resume position. The tests ended with no surviving processes or temporary files. The build passed 237 unit tests and 87 checks on the extracted offline package.

Download the complete Windows x64 offline ZIP, extract it fully, and double-click `RustView.exe` or `Start-RustView.cmd`. It includes 7-Zip, LibRaw, ImageMagick, ExifTool, and app-local VC++ runtime libraries for offline use from first launch. When moving to a new folder, close RustView and copy `RustView-data` from beside the previous executable to keep your settings, catalog, and processing records. [Offline ZIP](https://github.com/walker1911/RustView/releases/download/v0.6.5/RustView-0.6.5-win-x64-offline.zip) · [SHA-256 checksum](https://github.com/walker1911/RustView/releases/download/v0.6.5/RustView-0.6.5-win-x64-offline.zip.sha256)

[上个版本 / Previous release](https://github.com/walker1911/RustView/releases/tag/v0.6.4) · [离线版说明 / Offline guide](https://github.com/walker1911/RustView/blob/v0.6.5/OFFLINE.md) · [原生引擎源码 / Native sources](https://github.com/walker1911/RustView/blob/v0.6.5/docs/NATIVE-SOURCES.md)
