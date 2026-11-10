# Compatibility and scope / 兼容范围

RustView is an independent Windows x64 application, inspired by familiar classic-viewer workflows. It is not an ACD Systems product and does not link or redistribute ACDSee executable code, plug-ins, logos, private databases, or extracted resource inventories.

## What is retained

- Local image viewing and on-demand browsing inside ZIP / RAR / 7z, with password prompts. ZIP/7z modifications are backed up and verified. RAR is read-only.
- Manage, View, Compare and processing keyboard contexts; numeric keypad keys remain distinct from number-row keys. The project-authored [keyboard map](KEYBOARD.md) describes defaults. Custom JSON and structurally recognized REG imports cannot activate excluded commands.
- Smooth pointer-anchored zoom, panning, visible LOD detail tiles, linked/independent comparison of up to four images, virtualized management layouts, folder trees and bounded metadata sorting.
- GIF/WebP playback with frame timing, composition, disposal and loop handling. Complete animation export is available within the native processing budget. The static export option uses the first frame.
- RAW development parameters, high-depth processing, common filters, crop/resize/rotation, text/watermarks, processing records, presets, batch exports, and file/XMP operations. Processing errors can be bypassed to view the original without deleting the record.
- Ratings, labels, marks, collections, source EXIF/IPTC/XMP editing, RAW XMP sidecars, and contact-sheet printing. Catalog values and source metadata are separate; source writes require an explicit Save action.

## Deliberate exclusions

Slideshows/automatic image advance, calendars, disc-burning baskets, online services, and audio/video playback are outside scope. Image collections remain available. There is no split-volume or self-extracting archive support. Four unknown legacy commands and an unconfirmed 15-command context remain inactive. The compact functional table contains 257 identifiers: 206 retained, 32 excluded and 19 inactive. These counts do not mean every legacy behavior has received end-to-end GUI acceptance.

## Practical limits

- The interface is primarily Chinese. English documentation is provided; complete English localization is not yet available.
- The UI displays RGBA8. High-depth export reopens the original source; it cannot recover information missing from an 8-bit source. RAW source pixels are not overwritten. ICC profiles remain attached even when descriptive metadata is removed.
- LibRaw / native codecs determine camera support. Public samples cover Nikon Z6, Sony a7 III, Canon EOS R, iPhone 12 Pro ProRAW DNG and a libheif HEIC sample, not every brand/model/firmware. HEIC/HEIF is read-only in the selected native build; HDR gain maps and Apple editing semantics are not fully implemented.
- The independent subsystems have budgets: image LRU 768 MiB, GPU detail tiles 256 MiB, image worker tree 2 GiB, input/static RGBA 512 MiB, animation frame 128 MiB, native pixel cache 128 MiB and temporary disk cache 1 GiB. They are not a total application-memory ceiling. Oversized inputs and processing jobs can be refused.
- Indexes are bounded at 100,000 entries / 16 MiB; management sorting has a 64 MiB working budget and the folder tree is lazy and bounded. Clipboard exports use a persistent 2 GiB disk cache. Solid archives may require sequential decompression.
- Batch rename is one local set or one ZIP/7z transaction, not a transaction across multiple archives. Existing directory targets are not silently merged; reparse points are not followed. XMP companions are handled, while ambiguous shared-sidecar plans are refused.
- Common RGB/RGBA/grayscale TIFF pages are supported; unusual CMYK/multispectral/sample layouts may be rejected. Private ACDSee catalogs/development records are not imported. Unknown MFC customization blobs are not guessed.
- Native metadata in an archive is read-only. Very large metadata text becomes read-only in the UI. Some vendor-private tags may not round-trip through other applications.
- Printing provides margins, contact-sheet rows/columns, physical sizes, fit/fill, copies, ranges and text fields. It does not reproduce every decorative border/shadow/fade or legacy layout preset. Mounted device folders can be opened; PTP/TWAIN acquisition is not implemented.
- The algorithms, exact visual layout and every focus order are not pixel-identical to proprietary legacy software. Full native GUI interaction, every screen configuration and real-paper printing have not been validated. See [validation](VALIDATION.md).

## 中文摘要

保留压缩包看图、密码、常见图片和 RAW、GIF/WebP 动画、快捷键、平滑缩放、比较、管理、批处理、无损记录、元数据和打印。界面当前主要为中文。RAR 只读，不支持分卷、自解压包、原私有数据库或未经确认的隐藏命令；幻灯片、日历、刻录、在线与音视频继续排除。缓存和工作进程各有上限，不能把它们相加当作整个应用内存峰值。超预算会报告错误，并提供原图恢复入口。机型、界面细节、特殊 TIFF/HEIF 及打印样式仍有上述边界，测试结果与未完成的 GUI 验收分开记录。
