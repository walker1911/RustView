# Offline native sources / 离线版原生源码

The complete offline ZIP includes all native runtime dependencies. Source archives are provided for inspection, modification and rebuilding; **you do not need to download them to view photos offline**. RustView's original code is MIT licensed. Each native engine and its bundled libraries keep their own licenses and notices.

The matching ImageMagick and ExifTool source archives for v0.6.5 are available with their checksums on the v0.4.0 release. Preserve this guide, the original notices and the matching source materials when redistributing the complete offline package. You can replace the native libraries with compatible builds and run the viewer through the offline launcher. [RustView v0.4.0 source attachments](https://github.com/walker1911/RustView/releases/tag/v0.4.0)

## Included in the runtime ZIP

| Component | Corresponding source |
|---|---|
| 7-Zip 26.06 | `sources/7z2606-src.7z`, SHA-256 `e0364d39e8208557d939472c9a12d02ed1c12f45ca402e3b408bcaa63b36dd93`. Includes the original build instructions and notices. |
| LibRaw 0.22.3 | `native/LibRaw/LibRaw-0.22.3-Win64.zip`, SHA-256 `4e2213a0b10e32199a30b9114a61376cdb40f94df861262b4a6dca659701276b`. The official archive contains the source and Windows projects as well as the binaries. |
| ExifTool and Perl modules | The unchanged program and modules are retained in `native/ExifTool/exiftool_files`, including original licenses and `Licenses_Strawberry_Perl.zip`. Additional source attachments are described below. |
| Rust dependencies / fonts | Full notices and package/source inventory in `licenses/RUST-DEPENDENCIES.txt` and `.json`, plus the Rust toolchain attribution document. Exact application source is public on GitHub. |

7-Zip, ImageMagick and ExifTool are separate processes; LibRaw is loaded from `native/LibRaw/libraw.dll`. Close RustView and back up an engine before replacing it with an ABI/CLI-compatible x64 build. Retain ImageMagick's image-only `policy.xml`. The bootstrap launcher checks upstream hashes; use the offline launcher or `RustView.exe` directly with a reviewed replacement.

## ImageMagick and static codec libraries

Download [ImageMagick-7.1.2-35-Windows.7z](https://github.com/walker1911/RustView/releases/download/v0.4.0/ImageMagick-7.1.2-35-Windows.7z) and its `.sha256` from the v0.4.0 release. This is the unmodified official full Windows source distribution, including `Configure`, `Dependencies` and `ImageMagick`, rather than just the smaller main-source archive.

- Size: 769,375,980 bytes.
- SHA-256: `778bc4d5de0a8ae055284e85fd7dfff75dd44ec14c010cdf8dad5ab05f6a89d4`.
- All 38 component Git revisions declared in the shipped NOTICE match the archive; a complete CRC check passed.
- [Rebuild and replacement instructions](https://github.com/walker1911/RustView/blob/v0.4.0/docs/IMAGEMAGICK-REBUILD.md) and [component verification](https://github.com/walker1911/RustView/blob/v0.4.0/docs/IMAGEMAGICK-SOURCE-VERIFICATION.json).

Rebuilding this official native engine requires the Windows toolchain selected by its source projects, including Visual Studio 2026 C++ tools, SDK, ATL and MFC. Those development tools are not runtime prerequisites for the offline viewer. The source and build files were checked; a complete native rebuild and bit-for-bit reproducibility have not been claimed.

## ExifTool, Perl and GMP

Download [RustView-0.3.0-exiftool-sources.zip](https://github.com/walker1911/RustView/releases/download/v0.4.0/RustView-0.3.0-exiftool-sources.zip) and its `.sha256` from the v0.4.0 release. It contains the unmodified official ExifTool 13.62 source, GMP 6.1.2 source, Math::BigInt::GMP 1.6007 source/build rules, launcher-author CC0 example source, and original GCC toolchain notices. Individual source hashes and primary URLs are recorded in the bundle's `source-inventory.json` and the [verification inventory](https://github.com/walker1911/RustView/blob/v0.4.0/docs/EXIFTOOL-SOURCE-VERIFICATION.json).

The matching ExifTool/Perl/GMP source ZIP uses its original `0.3.0` filename. The direct link above downloads the source archive independently of the runtime ZIP.

- Size: 12,463,935 bytes.
- SHA-256: `194dbd0fb97b97a1ace2fa8ce0ac9fbdccf4f14906cbb738c5a67bfcecca87ed`.

The unchanged standard Perl/ExifTool distribution is provided under its Artistic option; obtain the standard versions from [ExifTool](https://exiftool.org/) and the [Strawberry Perl 5.32.1.1 x64 release](https://strawberryperl.com/release-notes/5.32.1.1-64bit.html). The runtime's original component notices remain in `exiftool_files/Licenses_Strawberry_Perl.zip`. GCC support DLLs accompany the linked Perl/XS combination under the GCC Runtime Library Exception, rather than being offered as an independent compiler-library product. GMP uses the LGPLv3 option; both the library and the corresponding XS application sources/build rules are provided for recombination.

To rebuild the optional GMP XS component, use the complete Strawberry Perl 5.32.1.1 x64 development distribution and an MSYS/MinGW environment. Build GMP 6.1.2 with `./configure --host=x86_64-w64-mingw32 --enable-static --disable-shared --prefix=/c/gmp`, then `make` and `make install`. In the matching Math-BigInt-GMP source directory run:

```cmd
perl Makefile.PL LIBS="-LC:/gmp/lib -lgmp" INC="-IC:/gmp/include"
gmake
gmake test
```

After reviewing the build, close RustView and back up/replace `native/ExifTool/exiftool_files/lib/auto/Math/BigInt/GMP/GMP.xs.dll` with `blib/arch/auto/Math/BigInt/GMP/GMP.xs.dll`. Retain its license and all other runtime files. The instructions follow the original build rules; a native rebuild has not been performed in this packaging environment. The author's small launcher CC0 source is supplied as an example, not asserted to reproduce the shipped launcher byte for byte.

## 中文说明

完整离线包已经包含全部运行依赖，**普通看图无需另外下载源码**。7-Zip 对应源码和 LibRaw 官方源码／二进制归档随运行包提供。v0.6.5 所用 ImageMagick 及其静态库的完整 Windows 源码、ExifTool／GMP／XS 补充源码与校验文件可从上方的 v0.4.0 Release 附件下载。ImageMagick 的 38 个组件版本与随包声明逐一核验一致。ExifTool 源码 ZIP 沿用原 `0.3.0` 文件名。各引擎和库保留各自的许可。

可以修改这些源码、重新编译并替换兼容的原生引擎；离线启动器直接执行主程序，不阻止兼容替换。重编所需的 C++ 工具链与看图所需运行库是两回事；完整离线包在首次启动时也无需下载运行依赖。
