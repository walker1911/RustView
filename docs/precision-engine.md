# 高位编辑与原生解码引擎

`precision::render` 在受限 worker 中直接从输入文件重新渲染；界面里的 RGBA8 预览不作为 RAW/16 位最终输出的来源。输入文件上限 512 MiB，原生像素缓存 128 MiB、禁用内存映射、临时像素磁盘缓存 1 GiB、最多 4 线程；同配置 ICC 编码保持确定性，RAW 已附工作 ICC 时避免重复身份转换；默认 PNG 按最终透明度选择 RGB/RGBA，保持显式 16 位而不额外增加透明通道。单次原生调用 120 秒，外层 worker Job 另限制整棵进程树 2 GiB 和整个任务时间。每幅高位 RGBA 画布最多 512 MiB；动画不在 Rust 中收集全部帧，原生处理达到磁盘限额会报错并保留原文件。

## 处理与保存

- RAW 使用 LibRaw 的相机白平衡或四通道手动白平衡、亮度、自动提亮、去马赛克、高光恢复、降噪、Gamma；输出通过固定 64 KiB 缓冲流式写入真正的 RGB16 PNG。
- 曝光、RGB 白平衡在对应的线性 ICC 中计算，再回到工作 ICC。sRGB、Adobe RGB、ProPhoto RGB 各自嵌入匹配原色及 LibRaw Gamma 曲线的 ICC，避免将宽色域像素误标为 sRGB。
- 普通 8/16 位图像使用 ImageMagick Q16-HDRI；支持色阶、亮度/对比/Gamma、饱和度/色相/通道混合、锐化、模糊、降噪、裁剪、旋转、镜头桶形校正、文字及图片水印，以及现有特效枚举。相近功能采用成熟滤镜，不能据此声称与 ACDSee 私有算法逐像素相同。
- 输出格式白名单：PNG、JPEG、TIFF、BMP、WebP、GIF、AVIF。完整动画只输出 GIF/WebP，保留逐帧时序和循环，零延时按查看器相同规则归一为 10 ms。动画帧上限 4096，当前帧模式只写当前指定帧。
- 显式色深优先于普通导出位深。精确 1/4/8 位灰度、4/8 位索引色输出 PNG；RGB555 输出实际 16 位 BMP 容器；16 位灰度和 48 位 RGB 输出 PNG/TIFF。容器冲突会报错。没有显式色深操作时使用导出设置中的 8/16 位。
- 保留元数据表示保留原生引擎可读取并输出的 EXIF/属性，不保证保留厂商全部私有 tag。取消后移除描述元数据；工作 ICC 始终保留，确保 Adobe RGB／ProPhoto 等像素仍被正确解释。ICC 编码逐标签重新做四字节对齐、规范 D50 白点和头部长度，已通过实际 PNG/LCMS 读取验证。RAW 源文件不可覆写；其它输出完成、校验并落盘后才备份和原子替换。首次原图备份在目标同级 `.rustview-originals`。

`recipes::Recipe` 保存最多 128 步编辑、RAW 参数和导出设置；`EditRecord` 另记录源文件绝对路径、长度/修改时间与可选包内条目名，不记录密码。加载记录时拒绝源文件已变更的情况。单条记录上限 1 MiB，预设库最多 128 项/16 MiB；记录不包含像素或撤销位图历史。

## 格式与安全边界

HEIC/HEIF（包括 `.hif`）、AVIF、JPEG XL 可通过内置 codec 解码，HEIC/HEIF 在本次官方 Windows 构建中为只读。苹果 ProRAW DNG 使用 LibRaw。常规 Rust 解码失败后只对受支持的图片扩展名尝试该备用引擎，先读尺寸并执行同样的像素/RGBA/输入预算检查。

原生引擎仅使用参数数组，无 shell。源文件复制到随机工作目录中的固定安全文件名；限制输出 coder；策略禁止外部 delegate、URL、PDF/PS、MSL/MVG coder 及 `@file`/管道读取；ICC/ICM 仅用于色彩管理。文字水印按字面处理 `@`、百分号和反斜线。超长参数使用应用内部生成并逐项引用的 ImageMagick 脚本，避免 Windows 命令行长度限制；不接受用户提供的脚本。

## 上游来源

- [ImageMagick 官方 Windows Q16-HDRI x64 7.1.2-35](https://github.com/ImageMagick/ImageMagick/releases/tag/7.1.2-35)；原始归档校验和、许可证及 delegate 通知在 `native/ImageMagick/SOURCE.txt`、`LICENSE.txt`、`NOTICE.txt`。本地精简仅移除了与 `magick.exe` 重复的命令入口；没有改动可执行文件。
- [ImageMagick 官方安全策略](https://imagemagick.org/script/security-policy.php)、[PNG/BMP 编码定义](https://imagemagick.org/defines/) 与[命令选项](https://imagemagick.org/script/command-line-options.php)。
- [LibRaw 官方 C API](https://www.libraw.org/docs/API-C.html) 与[输出参数](https://www.libraw.org/docs/API-datastruct.html#libraw_output_params_t)；运行库来源及双许可证见 `native/LibRaw`。
- [libheif 上游](https://github.com/strukturag/libheif)提供公开 HEIC 测试样本；公开样本的机型及测试范围见 `docs/VALIDATION.md`；真实相机照片不进入本仓库或发行包。

这些来源及少量公开样本只能验证此次实际测试的机型/编码，不代表支持所有固件、相机型号、HDR gain map 或 Apple 编辑语义。
