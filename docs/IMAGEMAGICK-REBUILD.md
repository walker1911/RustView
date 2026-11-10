# ImageMagick source and replacement / 源码与替换

The offline viewer uses the unmodified official **ImageMagick 7.1.2-35 portable Q16-HDRI x64** executable. ImageMagick and its codecs retain their upstream licenses; they are not covered by RustView's MIT license. The complete license and copyright texts are in `native/ImageMagick/NOTICE.txt` and `LICENSE.txt`.

## Corresponding source

The same RustView v0.3.0 Release provides **[ImageMagick-7.1.2-35-Windows.7z](https://github.com/walker1911/RustView/releases/download/v0.3.0/ImageMagick-7.1.2-35-Windows.7z)**. It is the unmodified official Windows source distribution, including the ImageMagick source, the Configure project, dependency sources, build files, and their Git metadata.

- Upstream: [immutable ImageMagick 7.1.2-35 Release](https://github.com/ImageMagick/ImageMagick/releases/tag/7.1.2-35).
- Size: **769,375,980 bytes**.
- SHA-256: `778bc4d5de0a8ae055284e85fd7dfff75dd44ec14c010cdf8dad5ab05f6a89d4`.
- All **38** component Git revisions named by the shipped NOTICE were checked against the archive and matched. A complete archive CRC test passed. The companion [verification inventory](IMAGEMAGICK-SOURCE-VERIFICATION.json) records each component and revision.

This source asset is for inspection, modification and rebuilding. It is **not required to run the offline viewer**. Merely downloading the smaller ImageMagick main-source archive does not supply the Windows dependency/build tree.

## Rebuild and replace

The source archive extracts three sibling directories: `Configure`, `Dependencies`, and `ImageMagick`. Keep them together. The upstream Windows build requires Visual Studio C++ tools, Windows SDK, ATL and MFC. This release's project selects Visual Studio 2026 / `v145`.

1. Open `Configure/Configure.sln` in Visual Studio and build **Release / x64**. Its project places the executable at `Configure/Artifacts/Release/x64/bin/Configure.Release.x64.exe`. Copy that executable to the `Configure` directory to match the upstream workflow's layout.
2. From the `Configure` directory run the configuration used by the official portable x64 HDRI workflow:

   ```cmd
   Configure.Release.x64.exe /noWizard /VS2026 /hdri /Q16 /x64 /static /linkRuntime
   ```

3. Open the generated solution in Visual Studio and rebuild **Release / x64**. The official workflow uses the Visual Studio developer command prompt and `msbuild /m /t:Rebuild /p:Configuration=Release,Platform=x64` from the parent directory. Modify a codec's included source before rebuilding to link the modified library into the resulting executable.
4. Close RustView, back up `native/ImageMagick/magick.exe`, and replace it with the compatible rebuilt `magick.exe`. Keep RustView's `policy.xml` and the support files. Run `RustView.exe` or the offline launcher's direct executable entry point.

RustView permits compatible native replacement and reverse engineering needed to debug library modifications. Its upstream-fetch bootstrap launcher checks pinned hashes and may reject a locally modified engine; use the offline package or run the executable directly for a reviewed local replacement.

These steps are based on the shipped project files and the [exact official release workflow](https://github.com/ImageMagick/ImageMagick/blob/7.1.2-35/.github/workflows/release.yml). A complete native rebuild has not been performed here; no bit-for-bit or Microsoft signing equivalence is claimed.

## 中文说明

离线运行包内已经包含解码引擎，**无需下载这个源码附件才能看图**。同一 Release 额外提供约 734 MiB 的官方 Windows 完整源码归档，供查看、修改和重新编译原生库；其摘要、38 个组件版本及归档完整性均已核验。请保留原许可证和版权声明。

需要重新编译时，准备 Visual Studio 2026 的 C++、Windows SDK、ATL、MFC，在解压出的 `Configure/Configure.sln` 中构建 Release/x64，再按上述官方参数生成并构建解决方案。可以修改其中的 LGPL 库后重新链接。关闭 RustView 后备份并替换 `native/ImageMagick/magick.exe`，保留项目的 `policy.xml` 与支持文件，直接运行离线版 `RustView.exe`。联网引导启动器会检查固定摘要；自行替换后请使用离线版或直接启动主程序。这里核验了源码与构建文件，未实际完成整个 ImageMagick 的重编译。
