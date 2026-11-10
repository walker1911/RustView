# 主力看图软件的取舍 / Rust viewer comparison

调研日期：2026-11-11。定位：自己的日常主力看图软件，重点是大量照片、尼康／索尼／佳能 RAW、苹果照片、压缩包、快速翻图、平滑缩放与稳定内存。下述他人功能来自项目维护者的 README／清单；未安装它们进行相同机器的性能对测，不把宣传当成性能结论。

## 看了哪些项目

| Rust 项目与主要来源 | 值得参考的已声明能力 | 对 RustView 的取舍 |
|---|---|---|
| [ViewSkater egui](https://github.com/ggand0/viewskater/blob/main/README.md)，[依赖与许可](https://github.com/ggand0/viewskater/blob/main/Cargo.toml) | 邻图预加载、按字节预算的 LRU、连续按键／滑杆浏览、双窗比较、光标附近显隐 UI；RAW 使用相机内嵌 JPEG。当前采用 egui 0.31 与 wgpu。 | 最接近本项目技术栈。参考加载优先级和连续浏览的体验；保留本项目的完整 RAW 解码与最多四图比较，不用内嵌 JPEG 取代 RAW 路径。MIT／Apache-2.0 双许可。 |
| [ViewSkater iced](https://github.com/ggand0/viewskater-iced/blob/main/README.md) | GPU 缓存、滑杆、ZIP/RAR/7z 浏览、双窗；README 已明确开发转向 egui 版本。 | 可以参考压缩包导航和大批图片性能测试的思路。不能把旧 iced 版支持的压缩包能力直接算作当前 egui 版已支持；也不引入第二套 UI 框架。MIT／Apache-2.0。 |
| [Oculante](https://github.com/woelper/oculante/blob/master/README.md)，[清单](https://github.com/woelper/oculante/blob/master/Cargo.toml) | 图片序列缓存、保持缩放与位置的比较、目录书签、像素／通道检查、非破坏编辑。README 当前写明维护模式、等待重写。 | 优先参考保持位置比较、文件夹访问和清晰的像素信息。现有编辑与比较功能继续完善即可，不复制一套编辑器或改用其 notan 渲染栈。MIT。 |
| [Lumia](https://github.com/iFence/Lumia/blob/master/README.md)，[清单](https://github.com/iFence/Lumia/blob/master/Cargo.toml)，[许可](https://github.com/iFence/Lumia/blob/master/LICENSE) | 进程隔离的专业格式预览；PSD/PSB 合成图；大图低清预览、磁盘像素缓存及可见区域分块；Windows 当前用户注册、由系统确认默认。README 区分了实现与产品目标。 | 值得研究“超过完整解码预算时仍可看”的磁盘分块路径。RustView 已有进程隔离和可见 GPU 分块，但目前完整源解码超限仍会拒绝，两者不等价。暂不引入其插件商店或 GPUI。MIT；每个原生依赖仍需单独核许可。 |
| [sriv](https://github.com/dllu/sriv/blob/main/README.md)，[清单](https://github.com/dllu/sriv/blob/main/Cargo.toml) | GPU 浏览、ICC/NCLX 色彩、物理像素倍率、多 DPI 表面回归、动画；另有语义搜索。项目仍标明进行中。 | 借鉴多屏 DPI／物理像素和色彩验证方法。暂不引入模型下载或语义搜索；本次读取的根目录和清单未发现明确许可声明，不能据此直接复制代码。 |
| [Emulsion](https://github.com/ArturKovacs/emulsion/blob/master/README.md)，[清单](https://github.com/ArturKovacs/emulsion/blob/master/Cargo.toml) | 轻量浏览方向；原仓库已明确停止开发。ViewSkater iced 的 README 说明滑杆受它启发。 | 仅作交互参考，不作为持续维护的核心依赖。MIT。分叉 README 中的“想做功能”不视为已经实现。 |

这些项目的 README 不是本项目的实现依赖。本轮没有复制它们的源码，也没有新增 Cargo 依赖。以后复用具体代码前应固定提交、确认对应文件许可与依赖成本，并保留通知。Windows／GPU／相机兼容性仍须用本项目真实流程验证。

## 这轮实际完成的范围

1. 平直 Picasa 式胶片带：去透视、保比例、卡片不叠压；保留弹簧运动、边缘渐隐和序号，不改变方向键或旧快捷键。
2. PSD／PSB：用已捆绑的 ImageMagick 读取首张合成图；普通浏览、缩略图、压缩包与导出均不把图层当动画。详见 [PSD 范围](PSD.md)。
3. 便携包附注册／卸载入口，图片候选关联与压缩包右键一次注册，默认选择交 Windows 确认。详见 [Windows 使用方法](WINDOWS-INTEGRATION.md)。

## 后续建议与验收目标

以下是结合你的使用方式作出的工程建议，不表示本轮已经实现，也不会自动加进程序。

| 优先级 | 建议 | 为什么适合主力使用、如何验收 |
|---|---|---|
| 第一批 | 固定大图库与快翻回归 | 已有 LRU、低清接力与独立进程。补连续正反向、长按、大跳转、坏图和密码等待的长时间测试；已显示图片后不回欢迎页，过期任务不覆盖当前图，停止后工作集回落。记录机器与样本，先测再定性能门槛。 |
| 第一批 | 显示实际缓存／任务占用并能取消 | 预算已经存在，但内存数值分别属于主进程、工作进程和 GPU，不能只显示一个误导数字。只按需打开诊断，不占用看图主界面；取消后子进程结束，临时文件可回收，密码不写日志。 |
| 第一批 | 大 PSD／RAW 的快速目标预览 | 目前缩略任务仍可能完整解码，遇复杂文档会触及 512 MiB／30 秒上限；主图可能成功而缩略失败。先研究现有原生引擎缩小解码／嵌入预览能力；目标低清图先到、高清后到，不能以取消预算换速度。 |
| 第一批 | 文件夹变化与失败恢复 | 外部编辑后刷新对应源、当前文件被移动时给明确状态；崩溃后保留可恢复的浏览位置与处理记录，单个坏图不拖死会话。需要检查现有刷新／记录路径，先补缺口而不是新增重复菜单。 |
| 第二批 | 多屏 DPI 和色彩回归 | 本项目已有源 ICC→显示预览、RAW 高位输出，但不等于显示器 ICC／HDR 全链路已验证。用 100/150/200% DPI、跨屏、旋转照片和透明图验证实际像素倍率、缩放锚点与色彩；先保证 SDR 正确，再考虑广色域／HDR。 |
| 第二批 | 常用文件夹与筛片便利 | 已有评级、标签、集合、文件管理；只在实际试用发现步骤过多时补目录书签或更快筛选，避免重复造目录库。对包内只读／提取状态保持明确。 |
| 研究项 | 超大图磁盘分块解码 | 可突破目前完整 RGBA 预算拒绝的限制，参考 Lumia 的磁盘缓存路径。成本包括各格式增量解码能力、磁盘限额、失效和异常清理；先用一类常见静态格式做端到端验证，不承诺所有 RAW／PSB 无限尺寸。 |

已经存在的 GIF/WebP 动画、比较、评级、标签、批量重命名、RAW 参数、高位导出和打印不重复列成“待新增功能”。幻灯片、日历、刻录篮子、在线服务、音频和视频继续排除。AI 搜索、云同步和大插件体系也不在这轮范围。

## English summary

For a daily primary viewer, prioritize continuous navigation, bounded resource use, reliable formats and reversible Windows integration. ViewSkater's egui edition is the closest UI stack; its older iced archive support must not be attributed to the new edition. Oculante offers useful inspection and comparison ideas but declares maintenance mode. Lumia's disk-backed large-image path is a useful future study, distinct from RustView's existing GPU tiles. sriv suggests concrete DPI/color regression practices; a clear source license was not found in the inspected root/manifest, so no code reuse is proposed. Emulsion is discontinued.

This is a primary-source feature review, not an installation or benchmark of those applications. The current scope adds a flat filmstrip, composite PSD/PSB viewing, and optional per-user Windows registration. Longer-term proposals above remain proposals. Existing features and the user's deliberate exclusions stay intact.
