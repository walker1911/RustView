# RustView

[![Windows x64 构建与测试](https://github.com/walker1911/RustView/actions/workflows/ci.yml/badge.svg)](https://github.com/walker1911/RustView/actions/workflows/ci.yml)

**打开压缩包，接着看。**

面向 **Windows x64** 的 Rust 看图项目。把相机 RAW、GIF/WebP 动画、Photoshop 合成图和 ZIP／RAR／7z 里的照片，放进熟悉的键盘翻图、Picasa 式胶片带与平滑缩放流程。

[下载 v0.4.0](https://github.com/walker1911/RustView/releases/tag/v0.4.0) · [English](README.md) · [兼容说明](docs/COMPATIBILITY.md) · [构建指南](BUILDING.md)

![RustView 项目概念图](assets/readme/hero.svg)

*配图为项目概念图，非真实窗口截图。v0.4.0 是早期公开预发行版。*

## 从照片出发

一整个压缩包的照片、一组相机 RAW、一个想仔细看清的动画，都应该能顺手打开。RustView 保留经典桌面看图的操作节奏：打开、翻图、缩放、比较，再继续下一张。

| 能力 | 可以做什么 |
|---|---|
| **直接看包内图片** | 打开 ZIP／RAR／7z，需要时输入密码；按需读取图片，不必先把整个压缩包展开。 |
| **连续翻图** | 左右键切换，上下键缩放；平直胶片带平滑跟随，当前图居中、最清晰，并显示文件名和 `1/84` 序号。 |
| **看 Photoshop 文档** | 直接读取本地及 ZIP／RAR／7z 内 PSD/PSB 保存的合成图，支持超过 512 MiB 的大源文件。 |
| **接纳现代格式** | 常见图片、LibRaw 支持的相机 RAW、苹果 ProRAW DNG，以及由内置引擎读取的 HEIC/HEIF、AVIF、JPEG XL。 |
| **平滑缩放与比较** | 保持鼠标指向位置缩放，拖动平移，按可见区域上传细节；最多四图独立或联动查看。 |
| **完整动画** | GIF/WebP 按帧时序、透明合成与循环信息播放；支持整段动画导出。 |
| **从原始源处理** | RAW 调节、裁剪、尺寸、色彩、锐化、文字与水印；导出真正的 16 位 PNG/TIFF，保存处理记录与预设。 |
| **整理与输出** | 批量改名前预览、文件夹与 XMP 伴随迁移、评级和标签、显式读写 EXIF/IPTC/XMP，以及接触表排版。 |

![压缩包浏览流程](assets/readme/archive-flow.svg)

快速翻图时，目标的低清预览已经缓存就先显示目标；目标尚无预览时保留上一张小马赛克并标明正在读取的目标文件，高清图就绪后接上。已经显示过图片后，加载、密码提示或读取错误不再把画面清成欢迎页。详见[浏览说明](docs/PICASA-BROWSING.md)。

处理步骤保存为记录，常用参数保存为预设。导出会重新读取源数据计算，界面中的 RGBA8 显示预览不作为 RAW／高位输出的来源。

压缩包密码仅用于当前会话。ZIP／7z 改包保留备份，RAR 保持只读。源图元数据与 RustView 自己的目录库分别存储；RAW 元数据写入 XMP 伴随文件。

## 内存有预算，读取按需进行

图像缓存有上限，读取和写入按队列进行；解码超时或超限以错误提示收尾。大图缩放优先上传屏幕可见的分块，减少一次性上传整图的等待。

缓存不会随浏览过的图片无限保留。解码使用独立、受限的 Windows 工作进程树；静态大图按可见范围上传细节，动画保留当前帧和一个预读帧。处理记录恢复失败时，可暂不应用记录而查看原图，记录仍保留。

| 资源 | 上限 |
|---|---:|
| 图像 LRU 缓存 | 768 MiB |
| GPU 细节分块 | 256 MiB |
| 图像工作进程树 | 2 GiB |
| 普通格式输入／静态 RGBA 输出 | 各 512 MiB |
| 验证后的本地及包内 PSD/PSB 源文件 | 64 GiB；像素预算继续生效 |
| 动画驻留 | 当前帧＋一个预读帧 |

![内存与工作进程结构](assets/readme/memory.svg)

这些是各层预算，**不是整个软件的总内存上限**。字体、驱动、概览纹理等还有额外开销；超限输入可能被拒绝。具体限额和实测范围见[验证记录](docs/VALIDATION.md)。

v0.4.0 对[本地及ZIP／RAR／7z内的PSD/PSB](docs/PSD.md)提供源文件例外：最多64 GiB。本地源直接定位合成图，包内所选项自动流式提取到临时盘并在任务结束后清理；RGBA和进程内存预算保持原值。极大的合成画布仍可能超限；包内提取需要临时盘容纳所选条目并额外保留 2 GiB，Solid 压缩包可能需要较长的顺序解压。

## 下载与运行

1. 下载 [Windows x64 完整离线包](https://github.com/walker1911/RustView/releases/download/v0.4.0/RustView-0.4.0-win-x64-offline.zip)。
2. 完整解压，双击 `RustView.exe` 或 `Start-RustView.cmd`。图片引擎和运行库已经包含，首次启动也无需联网或安装依赖。
3. 打开或拖入文件夹、图片、ZIP／RAR／7z。请保留完整目录。

离线 ZIP 提供配套 `.sha256` 校验文件。匹配的第三方源码单独放在同一 Release 中，普通看图无需下载源码附件。详见[离线版说明](OFFLINE.md)和 [v0.4.0 更新说明](docs/RELEASE-v0.4.0.md)。

设置、目录库和处理记录保存在程序旁的 `RustView-data` 中。普通运行不修改全局 PATH 或文件关联。可选运行 `Setup-File-Associations.cmd` 注册图片候选关联，以及 ZIP／RAR／7z 的 **Open with RustView** 右键入口；图片默认程序由 Windows 设置确认，压缩软件默认关联保持原样。详见[文件关联说明](docs/WINDOWS-INTEGRATION.md)。

### 顺手的快捷键

管理、查看、比较模式分别使用对应键位；数字键盘与主键盘数字区分别识别。支持自定义快捷键及可识别的 JSON／REG 配置导入。查看方向键不接管文本框或管理网格；若导入过完整旧配置，可在快捷键设置中补上新增方向键。

| 操作 | 默认按键 |
|---|---|
| 打开 | `Ctrl+O` |
| 进入查看／返回管理 | `Enter`／`Enter`、`Esc`、`Ctrl+W` |
| 上／下一张 | `←`／`→`；`PageUp`／`PageDown`；`Backspace`／`Space` |
| 平滑放大／缩小 | `↑`／`↓` |
| 缩放／适应／实际像素 | 数字键盘 `+`、`−`／`*`／`/` |
| GIF/WebP 暂停、继续、重播 | `G` |
| 重命名／比较所选图片 | `F2`／`Ctrl+Alt+R` |
| 属性／快捷键选项 | `Alt+Enter`／`Alt+O` |

完整默认键位见[快捷键参考](docs/KEYBOARD.md)。当前界面主要为中文；英文介绍不代表已经提供完整英文界面。

## 从源码构建

需要 Windows x64、Rust MSVC 工具链、包含 Windows SDK 的 C++ Build Tools，以及 PowerShell 7。测试、打包前先获取固定版本的原生引擎：

```powershell
git clone https://github.com/walker1911/RustView.git
cd RustView
pwsh -File .\tools\fetch-native.ps1
cargo build --release --locked
cargo test --locked -- --test-threads=2
pwsh -File .\tools\build.ps1
```

依赖、校验和与复现步骤见 [BUILDING.md](BUILDING.md)。

## 验证到哪一步

v0.4.0 单元测试 **208 项通过、2 项公开相机样本测试默认跳过**。发行检查覆盖静态图／压缩包、GIF/WebP 动画、PSD/PSB 合成图、缩略图、大型加密 RAR 取消清理，以及同一 Viewer 中的快速翻图。确切结果和限制见[更新说明](docs/RELEASE-v0.4.0.md)与[验证记录](docs/VALIDATION.md)。

真实相机样本包括 Nikon Z6、Sony a7 III、Canon EOS R、iPhone 12 Pro ProRAW 和 HEIC，不能推广到所有机型、固件与编码。已通过 UI 自动化核对原生窗口的密码重试、翻图与关闭；截图捕获仍不可用，完整视觉、多显示器、DPI 与实际纸张打印尚未全面验收。欢迎在 [Issues](https://github.com/walker1911/RustView/issues) 提供版本、格式／相机型号、预期行为和最小复现样本。

## 保持清楚的边界

不支持分卷、自解压包或 RAR 写入；不导入 ACDSee 私有数据库与开发记录，未确认语义的旧命令保持停用。幻灯片、日历、刻录篮子、在线服务与音视频播放不在项目范围内。详细行为差异见[兼容说明](docs/COMPATIBILITY.md)。

RustView 是独立项目，与 ACD Systems 无隶属关系。应用代码采用 [MIT 许可](LICENSE)；界面与解码复用 egui/wgpu、image、LibRaw、7-Zip、ImageMagick、ExifTool，各原生引擎保留自己的许可，详见[第三方声明](THIRD-PARTY-NOTICES.md)。
