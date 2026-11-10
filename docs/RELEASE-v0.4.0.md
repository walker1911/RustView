# RustView v0.4.0 / 更新说明

## 中文

**顺手翻图，直接看包内大 PSD。** v0.4.0 汇总 v0.3.0 之后的浏览体验、Photoshop 文档与 Windows 集成更新，继续以 Windows x64 完整离线包提供。这是早期公开预发行版，界面目前主要为中文。

### 浏览更连贯

- 查看模式下 `←`／`→` 切换上一张／下一张，`↑`／`↓` 平滑放大／缩小。原有快捷键、自定义键位和滚轮操作保留；管理网格、文本框和编辑／比较窗口继续使用各自按键。
- 底部为平直的 Picasa 式胶片带：保留照片比例，当前项居中、最大、最清晰，远处逐渐缩小、透明；显示文件名和 `1/84` 序号。点击缩略图即可切换。
- 连续翻图保留运动惯性，反向平滑转向，边缘渐隐；远距离跳转使用短交接。
- 已经显示过图片后，下一张读取期间保留低清接力画面。优先显示目标预览；目标尚无预览时保留上一张小马赛克并注明目标文件，高清就绪后替换。密码提示与读取失败也保留预览。预览不会被误作新图复制或编辑。

### 本地及压缩包内的大 PSD/PSB

- 新增 PSD/PSB 保存合成图查看，包括 ZIP／RAR／7z 内文档，不需要 Photoshop。图层不作为动画帧或多页。
- 本地及包内 PSD/PSB 源文件上限独立提升至 **64 GiB**，支持超过此前 512 MiB 输入限制的文档。源文件大并不等于画布大；像素、RGBA 与工作进程预算继续生效。
- 本地大文档定位合成图；包内仅将选中条目流式提取到私有临时盘，不必手动解包。临时盘需要条目空间加 2 GiB 余量。Solid 压缩包可能需要顺序解压。
- 加密包沿用会话密码提示。大型 RAR5 验证覆盖内容加密和文件名加密，包括物理体积超过 512 MiB 的密文包。
- 成功、失败、正常取消后清理提取文件；快速翻图隔离过期任务。解码器的临时文件也归入读取任务统一回收，修复探测阶段取消后遗留小目录的问题。关窗先取消并等待资源清理，修复窗口关闭事件互相抵消、主进程未退出的问题。
- PSD/PSB 图像内容只读，元数据写入入口拒绝修改文档；可另存 PNG/TIFF 等已有格式，或保存处理记录。评级与分类可留在本地目录库。请在 Photoshop 保存时包含兼容合成图；复杂效果不保证与 Photoshop 完全一致。

### 可选 Windows 集成

完整解压到准备长期使用的位置后，可运行 `Setup-File-Associations.cmd`。它注册当前用户的图片候选关联，以及 ZIP／RAR／7z 的 **Open with RustView** 右键入口；Windows 11 可能显示在“显示更多选项”中。图片默认程序由 Windows 设置确认，不接管压缩软件默认关联。可用 `Remove-File-Associations.cmd` 移除；移动便携目录后重新注册。

### 离线包与范围

下载 **`RustView-0.4.0-win-x64-offline.zip`**，完整解压，运行 `RustView.exe` 或 `Start-RustView.cmd`。7-Zip、LibRaw、ImageMagick、ExifTool 及应用本地 VC++ 运行库全部包含，首次运行无需联网或安装依赖。保留完整目录；设置与目录库在旁边的 `RustView-data` 中。更新前关闭旧程序并备份该目录，不要把个人数据上传到源码仓库。

相机 RAW、苹果 ProRAW DNG／HEIC、WebP 与 GIF 动画沿用已有解码和预算机制。当前范围仍不包括分卷／自解压包、RAR 写入、ACDSee 私有数据库导入，以及幻灯片、日历、刻录、在线服务、音视频播放。

### 验证

最终 v0.4.0 单元测试 **208 项通过、2 项公开相机测试默认跳过**；独立离线解压目录的 **217 项检查全部通过**，包括209项工作进程回归和8项同Viewer快速导航。其中76项覆盖大型加密RAR，包含16次实际取消、观测进程退出与临时目录清理。系统PATH与不可用代理下完成图片、动画、RAW及PSD回归；ZIP完整性及逐文件摘要通过。详见 [验证记录](VALIDATION.md)，最终归档摘要见配套 `.sha256`。

内存为分项预算：主工作进程树 2 GiB、静态 RGBA 512 MiB、图像 LRU 768 MiB、GPU 细节分块 256 MiB，预览与动画另有界限。这不是整个 GUI 的总内存上限，也不保证任意文档都可解码。原生窗口密码重试、翻图与关闭已有 UI 自动化核对；截图接口不可用，多 DPI、多显示器、长时间完整图库和实际纸张打印仍未全面验收。

## English

**Keep the flow. Open large Photoshop documents inside archives.** v0.4.0 brings together the browsing, PSD/PSB and Windows-integration updates since v0.3.0. This is an early public prerelease for Windows x64; the application UI is currently primarily Chinese.

### Browsing

- In single-image View, `←` / `→` select the previous / next image and `↑` / `↓` zoom smoothly. Existing shortcuts, custom bindings and mouse-wheel zoom remain available. Management, text fields, editing and comparison retain their own key handling.
- A flat Picasa-style filmstrip preserves photo proportions. The selected image stays centered, largest and clearest; distant items shrink and fade. Click to navigate; the filename and a `1/84` counter identify your position.
- Continuous navigation preserves motion, reverses smoothly and fades at the edges. Distant jumps use a short handoff.
- Once a picture has been shown, loading keeps a tiny continuity preview. It uses the target's cached preview when available; otherwise it retains a mosaic of the previous image, labeled with the pending target, until the full image arrives. Password prompts and errors keep that preview. Preview pixels are not treated as a newly loaded editable source.

### Large PSD/PSB, including archive entries

- View saved Photoshop composites locally and inside ZIP / RAR / 7z, without Photoshop. Layers are not interpreted as animation frames or pages.
- Validated local and archived PSD/PSB sources can be up to **64 GiB**, beyond the former 512 MiB source limit. Decoded-pixel, RGBA and worker-memory budgets still apply; very large composites may be refused.
- Local sources seek to the composite. Only the selected archive entry is streamed to private temporary disk, with no manual extraction. Temporary storage must fit the entry plus a 2 GiB reserve. Solid archives may require sequential decompression.
- Session password prompts also cover encrypted archives. Large-RAR5 regressions include data-only and header-and-data encryption, with physical ciphertext containers above 512 MiB.
- Success, errors and normal cancellation clean extracted files. Codec scratch files are also owned by the supervised request, fixing small directories left by cancellation during probing. Rapid navigation isolates late results. Closing cancels work and waits for resource cleanup; this fixes a close-event loop that could leave the application process alive.
- PSD/PSB image contents are read-only, and metadata-writing entry points reject document changes. Export a flattened result to an existing format such as PNG/TIFF or save a processing record. Local catalog ratings/categories remain available. Save a compatible composite in Photoshop; exact rendering of complex effects is not guaranteed.

### Optional Windows integration

Place the complete portable folder in its intended location, then run `Setup-File-Associations.cmd` for per-user image candidates and **Open with RustView** context-menu entries on ZIP/RAR/7z. Windows 11 may show the verb under **Show more options**. Confirm image defaults in Windows Default Apps; archive-application defaults are preserved. Remove registration with `Remove-File-Associations.cmd`, and register again after moving the portable folder.

### Complete offline download

Download **`RustView-0.4.0-win-x64-offline.zip`**, fully extract it and run `RustView.exe` or `Start-RustView.cmd`. 7-Zip, LibRaw, ImageMagick, ExifTool and app-local VC++ runtimes are included. First launch requires no network connection or dependency installation. Keep the full folder. Data stays in `RustView-data` beside the executable; close the older viewer and back up that data before updating.

Existing camera RAW, Apple ProRAW DNG/HEIC and GIF/WebP animation support is retained. Split/self-extracting archives, RAR writing, ACDSee private-database import, slideshows, calendars, disc burning, online services and audio/video playback remain outside scope.

### Validation

The final v0.4.0 unit suite passes **208 tests**, with **2 public-camera tests ignored by default**. All **217 checks** pass from an independent offline extraction: 209 worker cases plus eight same-Viewer navigation cases. These include 76 large encrypted-RAR checks with 16 actual cancels, observed process termination and temporary cleanup. Images, animation, RAW and PSD are tested with system-only PATH and unavailable proxies; ZIP CRC and per-file hashes pass. See [VALIDATION.md](VALIDATION.md) and the final archive's accompanying `.sha256` file.

Memory limits are subsystem budgets: a 2 GiB main worker tree, 512 MiB static RGBA output, 768 MiB image LRU and 256 MiB GPU detail tiles, with separately bounded previews and animation. These do not cap total GUI memory or guarantee decoding of every document. Native-window password retry, navigation and close were checked with UI automation. Screenshot capture is unavailable; full visual, DPI, multi-monitor, long-library-session and paper-printing acceptance remains incomplete.

## Sources and licenses / 源码与许可

The release includes matching source attachments for unchanged native engines: **`ImageMagick-7.1.2-35-Windows.7z`** and **`RustView-0.3.0-exiftool-sources.zip`**, with `.sha256` files. The ExifTool attachment keeps its historical name because its contents are unchanged. 7-Zip source and the official LibRaw source/binary archive are inside the runtime ZIP. These source downloads are optional for viewing. Application code is MIT; bundled components retain their own licenses. See [OFFLINE.md](../OFFLINE.md), [native source guide](NATIVE-SOURCES.md) and [third-party notices](../THIRD-PARTY-NOTICES.md).

同一 Release 提供上述匹配原生源码及校验文件；ExifTool 附件沿用历史文件名。普通离线看图只需下载完整离线运行包。应用源码为 MIT，各原生引擎及其依赖保留各自许可。
