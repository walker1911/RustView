# PSD / PSB 文档预览 · Photoshop document viewing

## 中文

RustView 可以直接浏览 `.psd` 和 `.psb`，包括 ZIP／RAR／7z 内的文档。与普通图片一样，可翻图、平滑缩放、生成底部预览；加密压缩包沿用现有密码提示。完整离线包使用随附的 ImageMagick Q16-HDRI x64，不需要 Photoshop 或新下载的插件。

查看的是保存的**文档合成图**。RustView 按文件索引跳过图层数据，直接读取 RGB8 的 RAW／RLE 合成图。ZIP（包括预测编码）合成图在受限工作进程里逐行解压，再由随附引擎处理颜色、透明度和位深。导出或恢复编辑记录也只处理一张合成图。

已用公开的合成小样验证：RGB8、RLE/PackBits、RGBA 透明合成、RGB16、CMYK8、嵌入 sRGB ICC，以及带独立图层的 PSD／PSB。正常查看使用现有 ICC→sRGB 链路，屏幕预览为 RGBA8；读取 16 位输入不表示显示器预览是 16 位。带有效 ICC 的 CMYK 依照该配置转换；没有配置的 CMYK 使用引擎的默认转换，不能据此保证印刷色准。

PSD/PSB 读取不改动源文档。原生安全策略只允许读取 PSD/PSB；应用拒绝向 `.psd` / `.psb` 目标保存。元数据读取继续可用；属性写入入口对 PSD/PSB 拒绝 Auto、Embedded、Sidecar 三类请求，也检查 `8BPS` 文件签名，避免改后缀绕过。此入口不生成文档属性 XMP；已有 XMP 仍可读取。本地目录库中的评级、分类等整理信息，以及用户明确执行的文件移动、重命名、删除，不受这项图像内容只读限制影响。需要保存合成结果时，另存 PNG、TIFF 等既有输出格式，或保存编辑记录。没有增加 Photoshop 图层、文字、智能对象或效果编辑器。

v0.4.0 预发布版已将PSD/PSB的源文件上限与像素内存预算分开：本地文档和ZIP／RAR／7z内的PSD/PSB条目均最多64 GiB。大于512 MiB的本地源验证文件头后直接定位合成图，不先复制整个文档。包内源由随附7-Zip自动按需提取**所选一项**到私有临时文件，再定位合成图；不需要手动解包，不展开整个包。

文档文件很大不等于合成画布很大；画布在预算内时仍显示完整像素，并非降低清晰度的预览。大条目提取使用固定64 KiB传输缓冲，先检查索引大小，解压开头验证PSD签名和版本，实际解出长度必须与索引一致。临时盘需要容纳所选条目并额外保留2 GiB；每16 MiB复查余量，空间不足会停止并清理半成品，不把整个条目读进内存。

RGBA 缓冲仍为512 MiB；普通解码子进程树2 GiB，缩略图子进程树512 MiB；任务可取消并有超时。ImageMagick 自身128 MiB内存缓存、1 GiB磁盘缓存和尺寸限制继续生效。极大的合成画布仍可能因预算被拒绝，64 GiB源文件限额不意味着可解码同等大小的像素。

解出临时文件在任务成功、错误和正常取消后清理，不建立长期的完整PSD解包缓存。解码子进程的TEMP/TMP也指向该任务目录，取消并终止子进程后仍由外层回收探测与解码临时文件。后台胶片带不重复提取大PSD；管理缩略图只为当前选中的大条目生成，已经显示的图像沿用有界缓存和低清接力。Solid RAR/7z仍可能需要顺序解压较前面的数据，首次读取耗时取决于压缩方式和存储速度。密码沿用现有会话提示与处理，不保存在磁盘或日志中。

其他格式仍有512 MiB输入上限；属性面板的元数据读取及部分文件复制操作也保留各自512 MiB上限。PSD/PSB图像源的例外不放宽这些独立预算。

缺少有效合成图时，简单文档可在预算内尝试合成。复杂文档若有 Photoshop 内嵌 JPEG，查看模式会明确标为「PSD 内嵌预览」，只显示该缩略图。需要完整清晰度时，请在 Photoshop 的「文件处理」中启用「最大兼容性」并另存副本。

回归样本及来源记录在源代码的 `tests/fixtures/psd/README.md`（运行包不附测试夹具）。单元测试断言实际像素、透明度、首图选择、内存预算拒绝、禁止写 PSD，以及单帧导出；源码工具 `tools/psd_smoke.py` 走实际受限子进程，覆盖本地文件、缩略图、压缩包列举、加密7z及合成图导出。

2026-11-11 大源文件修订验证：198项单元测试通过、2项跳过；103项既有真实进程回归通过。另用一份本地约1.98 GiB、900×1200的PSD验证8条真实流程（打开、缩略图、单页、色彩、区域预览、原图、渲染预览和无修改编辑）。该样本打开耗时约1.3秒，探测进程及其解码子进程的私有提交内存采样峰值约40 MiB；这不是GUI总内存或所有文档的性能承诺。文件大小、修改时间和分段指纹保持相同。私人文档及其派生图像不随源码或运行包分发。

2026-11-11 包内大PSD修订：204项单元测试通过、2项跳过；35项新增真实进程测试全部通过。自产条目546,000,130字节，覆盖ZIP、RAR5 stored、7z及AES ZIP/7z的列举、完整合成图、缩略图、单页和PNG16导出。密码缺失/错误/正确、普通格式限额和正常/错误/取消清理均验证；取消发生在部分条目已写入临时盘后。该轮RAR密码只使用小型上游夹具，大型加密RAR的后续补测见下文。测试源与各压缩包哈希不变，独占临时目录最终为空。上述小画布测试的进程树私有提交采样最高约15.84 MiB，不含OS文件缓存，不能外推为所有PSD的内存用量。复测工具见源码 `tools/large_archive_psd_smoke.py`，生成样本及来源见 `tests/fixtures/psd/LARGE-ARCHIVES.md`；大样本不随运行包或仓库分发。

### 大型加密RAR与连续翻图补测

已另用官方RAR 7.23测试工具生成RAR5文件数据加密（`-p`）和文件名及数据加密（`-hp`）两类样本。每张自产PSD为546,589,948字节，保存的合成图512×384且A/B颜色不同。除压缩型小容器外，还有**物理体积546,590,398／546,590,638字节**的存储型加密包，避免把“解出后大”误称为“大体积密文已测”。工具和数据仅用于本地测试，不随RustView分发。

76项真实进程测试覆盖缺失／错误／正确密码、列举、完整查看、缩略图、单页、PNG16两色标记，以及重复取消后重新读取。对观测到的后代进程持有句柄并等待退出，同时逐项确认独占临时目录为空；单看目录为空不能代替进程清理检查。早取消要求实际观察到尚未完成的临时PSD，保留原有内存限额。

另有8项同一Viewer导航回归，按10ms／250ms间隔连续翻图，运行真实导航动作、队列、poll、draw_image与读取进程，核对最后图片像素、过期任务隔离、已有图像后不出现欢迎页、缓存预算和中途关窗清理。压缩型样本在两张大PSD之间切换，物理大存储型样本在大PSD与seed图之间切换；这些是无原生窗口的应用队列／绘图测试，不能冒充完整窗口视觉验收。真实窗口还验证了密码重试、切换与原生关闭行为；截图接口仍受限，UIA文本验证不等于截图检查。

退出处理现在先取消任务，等读取、动画和封面资源退休后再关闭。等待帧只发送CancelClose，完成帧只发送Close，避免原生关闭事件被反复抵消。当前单元回归208项通过，2项跳过。内存记录是特定512×384测试样本的进程树私有提交采样，不含GPU或可回收系统文件缓存，也不是整个实际窗口的总内存上限。复测方式和样本来源见源码 `tools/encrypted_rar_psd_smoke.py`、`tools/navigation_smoke.py` 和 `tests/fixtures/psd/ENCRYPTED-RAR.md`。

## English

RustView opens PSD and PSB documents directly, including entries inside ZIP, RAR and 7z archives. It reuses the bundled offline ImageMagick Q16-HDRI x64 engine and existing navigation, zoom, thumbnail and archive-password flows.

RustView seeks past the layer data and reads the saved composite. RGB8 raw and PackBits composites have a direct path. ZIP composites, including predicted ZIP, are decompressed row by row in a bounded worker and passed to the bundled engine for color, alpha and depth handling. Exports and restored edits use a single composite.

Synthetic regression images cover raw and RLE RGB, composite alpha, 16-bit RGB, CMYK, embedded sRGB ICC and layered PSD/PSB. Viewing converts through the existing ICC-to-sRGB path to an RGBA8 display preview. Untagged CMYK uses the engine's default conversion and does not establish print-color accuracy.

PSD/PSB source documents are preserved. The native policy grants read access only, and application exports reject PSD/PSB destinations. Metadata remains readable, while the metadata-write entry point rejects Auto, Embedded and Sidecar requests for PSD/PSB, including renamed documents detected by their `8BPS` signature. It does not create a metadata XMP for these documents; existing XMP files can still be read. Ratings/categories in the local catalog and explicit file move, rename or delete operations remain available. Save a flattened result to an existing output format such as PNG/TIFF, or save an edit record. This is not a Photoshop layer editor.

The v0.4.0 prerelease accepts local PSD/PSB sources and PSD/PSB entries in ZIP/RAR/7z up to 64 GiB, independently of the decoded-pixel budget. Large local sources require a valid Photoshop signature/version and are read directly through the composite scene. Archive browsing automatically streams only the selected entry to a private temporary file with a fixed 64 KiB transfer buffer. It checks the advertised length, validates the Photoshop header early, and requires the actual extracted length to match. No manual extraction is needed. A large layered container with a small composite still displays at full resolution.

The 512 MiB RGBA budget, 2 GiB main worker, 512 MiB thumbnail worker, native 128 MiB memory/1 GiB disk cache and timeouts remain in place. Very large composites can still be rejected. Before extracting a large entry, its temporary volume must have room for that entry plus a 2 GiB reserve; free space is checked again every 16 MiB. Success, errors and normal cancellation clean the task's temporary files. There is no persistent cache of complete extracted PSDs, and background filmstrip work avoids duplicate large extractions. Solid archives may require sequential decompression. Other image formats, metadata reads and some file-copy operations retain their separate 512 MiB limits.

For a complex document with no usable full composite, the viewer can display its embedded Photoshop JPEG and label it as a preview. Simple documents may still be reconstructed within the resource budget. For full-resolution viewing, enable Photoshop's Maximize PSD and PSB File Compatibility setting and save a copy.

Large encrypted RAR5 is now covered separately using official RAR-generated data-only and header-and-data encryption, including two physical ciphertext containers above 512 MiB. The 76 supervisor/worker cases check passwords, full composite pixels, exports, cancellation, observed process termination and temporary cleanup. Eight same-Viewer headless cases additionally exercise navigation at 10/250 ms, late-result isolation, pixel identity, continuous presentation, cache limits and close during partial extraction. Native-window password/keyboard/exit acceptance is separate; it does not establish screenshot-based visual quality. Exit now cancels and drains existing resources before closing, with mutually exclusive Close/CancelClose frames. The RAR fixture writer and generated archives are test-only and are not distributed with RustView.

## Primary implementation references

- [Adobe Photoshop file format specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/): image resources, composite data, alpha and PSD/PSB section sizes.
- [ImageMagick 7.1.2-35 PSD coder](https://github.com/ImageMagick/ImageMagick/blob/7.1.2-35/coders/psd.c): scene-zero composite reading, layer skipping and fallback layer reconstruction.
- [ImageMagick security policy](https://imagemagick.org/security-policy/): coder permissions and resource limits.
