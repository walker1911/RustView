# Building RustView / 构建 RustView

The **v0.7.1** Windows x64 release provides a complete offline ZIP with the OCR engine and Simplified Chinese and English models. The source guide lists the matching native source archives. [Native sources](docs/NATIVE-SOURCES.md)

**v0.7.1** 提供 Windows x64 完整离线包，含 OCR 引擎和简体中文、英文模型，解压即可运行。[原生引擎源码](docs/NATIVE-SOURCES.md)

## Windows x64 prerequisites

- Rust stable with the `x86_64-pc-windows-msvc` target and Visual Studio C++ Build Tools / Windows SDK. The development release was tested with Rust 1.99.1.
- CMake 3.22 or newer and Ninja for building the portable OCR helper.
- Windows PowerShell 5.1 or PowerShell 7. The scripts unpack engines into the project folder. They do not install system-wide software or change system PATH, execution policy or registry settings.
- An Internet connection for Cargo dependencies and the first native-engine preparation. Once prepared, normal image browsing works offline.

The public source repository contains application source, small generated test fixtures, native policy, provenance and license text. Downloaded engines, compiler tools, caches, runtime user data and camera sample photos are excluded.

## Prepare, build, test

From the repository root:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\fetch-native.ps1
.\tools\build-ocr.ps1
cargo build --release --locked --target x86_64-pc-windows-msvc
cargo test --locked -- --test-threads=2
```

`-ExecutionPolicy Bypass` affects only this PowerShell process; it does not change the machine's execution policy. `fetch-native.ps1` downloads fixed official releases and checks their SHA-256 digests against [native/manifest.json](native/manifest.json) before executing an extractor or copying runtime files. It uses a pinned official `7zr.exe` to extract the 7-Zip package, then that package's `7z.exe` for the other archives. It never runs an installer. The project's image-only ImageMagick policy replaces the upstream default policy.

For a diagnostic or offline cache check:

```powershell
.\tools\fetch-native.ps1 -VerifyOnly
```

This checks all cached archive digests, the core runtime digests and required support files. Normal preparation also compares generated runtime/support files with the extracted archive; differing existing files are refused, not silently replaced. The policy, provenance and license templates come from the source checkout, so ordinary Git line-ending conversion does not cause a false archive mismatch. For isolated validation, `-DestinationRoot .\test-output\native-check` prepares a separate native tree; `-CacheDirectory` can reuse an existing verified cache.

`build-ocr.ps1` verifies pinned Tesseract and Leptonica sources and the English and Simplified Chinese language models before building. The Windows x64 helper and models are included in both packages. They run on demand and need no compiler, system OCR language packs, or Internet connection at runtime. See [OCR sources and licenses](native/Tesseract/SOURCE.txt).

Tests use small generated images and licensed libarchive fixtures. Fixture passwords in `tests/fixtures/README.md` are public test constants. The two public-camera tests are opt-in and need separately downloaded samples; they are not required for an ordinary checkout. See [validation](docs/VALIDATION.md) for version-specific results and the GUI checks that have not been completed.

## Public bootstrap package

```powershell
.\tools\package-public.ps1
```

The script writes `dist/RustView-0.7.1-win-x64-bootstrap.zip` and its `.sha256`. `-SkipBuild` packages the most recent existing Windows x64 release executable. `-OutputRoot .\test-output\public-package` keeps a validation package separate from existing releases.

Extract the ZIP, then double-click **Start-RustView.cmd**. On the first launch a visible PowerShell process downloads and verifies the official native engines. After preparation, the viewer starts; later launches work offline. Preparation failure is displayed and can be retried. App-local Microsoft VC++ runtime DLLs are included with Microsoft attribution; no VC installer is run. Keep the complete extracted folder.

The bootstrap ZIP includes the portable OCR helper and models, RustView, the two preparation scripts, pinned native metadata, policy, upstream license notices and project documentation. The separately available complete offline ZIP also includes the four preassembled image and archive engines. Their third-party dependencies are not uniformly MIT licensed.

## Complete offline package

After native preparation:

```powershell
.\tools\build.ps1 -Offline
```

The script writes `dist/RustView-0.7.1-win-x64-offline.zip` and its `.sha256`. Extract the complete ZIP and launch `RustView.exe` or `Start-RustView.cmd` to browse offline. The package includes all four native image and archive engines, the OCR helper and models, app-local Microsoft VC++ libraries, Rust and font license notices, matching 7-Zip source, and LibRaw's official source and binary archive. [Offline guide](OFFLINE.md)

Download the remaining corresponding source and Windows rebuild materials from the links in [docs/NATIVE-SOURCES.md](docs/NATIVE-SOURCES.md). They are not needed for normal offline viewing. When redistributing the complete offline package, also provide those source materials and preserve the notices, including the static dependencies listed in ImageMagick's `NOTICE.txt`. A source download and the tools required to rebuild an engine are separate from the runtime requirements for viewing photos.

Running `tools/build.ps1` without `-Offline` produces `RustView-0.7.1-win-x64.zip`. `-SkipBuild` reuses an existing x64 release executable. Use `-OutputRoot` to put validation packages in a separate folder.

Both packaging scripts locate MSVC x64 redistributables through Visual Studio's `vswhere` or `VCToolsRedistDir`. If that workload is absent, pass `-RuntimeDirectory` pointing to a permitted `Microsoft.VC143.CRT` directory. Available upstream redist license text is copied alongside Microsoft attribution. No personal installation path is written into the package.

The scripts use system `cargo`, then an explicitly configured `CARGO_HOME`, or the development workspace's optional `.tools` Rust installation. They do not require the original developer's drive layout. Archives and generated user data are not placed in Git.

## Native replacement and sources

The versions, immutable URLs and archive/core digests are in `native/manifest.json`. Individual `native/*/SOURCE.txt`, licenses and ImageMagick's complete `NOTICE.txt` retain upstream source references. LibRaw is dynamically loaded from `native/LibRaw/libraw.dll`; an ABI-compatible x64 DLL can be substituted for a local build. 7-Zip, ImageMagick and ExifTool run as separate processes. A locally substituted engine changes the pinned runtime digest. The bootstrap launcher reports that difference; the offline launcher runs `RustView.exe` directly without enforcing the original engine digest. See [third-party notices](THIRD-PARTY-NOTICES.md).

## 中文说明

准备 Windows x64 的 Rust MSVC 工具链、Visual Studio C++ Build Tools、Windows SDK、CMake 和 Ninja，然后在仓库根目录执行上面的准备、构建和测试命令。离线 OCR 程序与简中、英文模型会随包提供。脚本兼容 Windows PowerShell 5.1 与 PowerShell 7，首次准备时需要联网，软件和系统设置由用户管理。

构建脚本支持**完整离线包**和 **bootstrap 薄包**。离线包完整解压后双击 `RustView.exe` 或 `Start-RustView.cmd`，首次启动也无需联网；薄包首次运行启动器时联网下载并校验依赖，之后离线看图。两种版本都需保留完整目录，不要仅复制单个 EXE。

`tools/package-public.ps1` 生成薄包；`tools/build.ps1 -Offline` 在依赖已准备后生成完整离线包，两者输出互不覆盖。第三方引擎及其静态依赖保留各自许可；对应源码和重编材料的下载地址见[源码说明](docs/NATIVE-SOURCES.md)。普通使用者无需下载源码即可离线看图。仓库与发行包排除用户数据库、编辑记录、剪贴板缓存、工具链及相机样本。
