# Building RustView / 构建 RustView

## Windows x64 prerequisites

- Rust stable with the `x86_64-pc-windows-msvc` target and Visual Studio C++ Build Tools / Windows SDK. The development release was tested with Rust 1.99.1.
- Windows PowerShell 5.1 or PowerShell 7. The scripts unpack engines into the project folder. They do not install system-wide software or change system PATH, execution policy or registry settings.
- An Internet connection for Cargo dependencies and the first native-engine preparation. Once prepared, normal image browsing works offline.

The public source repository contains application source, small generated test fixtures, native policy, provenance and license text. Downloaded engines, compiler tools, caches, runtime user data and camera sample photos are excluded.

## Prepare, build, test

From the repository root:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\fetch-native.ps1
cargo build --release --locked --target x86_64-pc-windows-msvc
cargo test --locked -- --test-threads=2
```

`-ExecutionPolicy Bypass` affects only this PowerShell process; it does not change the machine's execution policy. `fetch-native.ps1` downloads fixed official releases and checks their SHA-256 digests against [native/manifest.json](native/manifest.json) before executing an extractor or copying runtime files. It uses a pinned official `7zr.exe` to extract the 7-Zip package, then that package's `7z.exe` for the other archives. It never runs an installer. The project's image-only ImageMagick policy replaces the upstream default policy.

For a diagnostic or offline cache check:

```powershell
.\tools\fetch-native.ps1 -VerifyOnly
```

This checks all cached archive digests, the core runtime digests and required support files. Normal preparation also compares generated runtime/support files with the extracted archive; differing existing files are refused, not silently replaced. The policy, provenance and license templates come from the source checkout, so ordinary Git line-ending conversion does not cause a false archive mismatch. For isolated validation, `-DestinationRoot .\test-output\native-check` prepares a separate native tree; `-CacheDirectory` can reuse an existing verified cache.

Tests use small generated images and licensed libarchive fixtures. Fixture passwords in `tests/fixtures/README.md` are public test constants. The two public-camera tests are opt-in and need separately downloaded samples; they are not required for an ordinary checkout. See [validation](docs/VALIDATION.md) for the actual v0.3 results and the GUI checks that have not been completed.

## Public bootstrap package

```powershell
.\tools\package-public.ps1
```

Output: `dist/RustView-0.3.0-win-x64-bootstrap.zip` and its `.sha256`. `-SkipBuild` packages the most recent existing Windows x64 release executable. `-OutputRoot .\test-output\public-package` keeps a validation package separate from existing releases.

Extract the ZIP, then double-click **Start-RustView.cmd**. On the first launch a visible PowerShell process downloads and verifies the official native engines. After preparation, the viewer starts; later launches work offline. Preparation failure is displayed and can be retried. App-local Microsoft VC++ runtime DLLs are included with Microsoft attribution; no VC installer is run. Keep the complete extracted folder.

The public ZIP intentionally does **not** redistribute native engine binaries. It includes RustView, the two preparation scripts, pinned native metadata, policy, upstream license notices and project documentation. ImageMagick's portable build contains third-party dependencies with their own licenses; fetching the unmodified official package directly keeps the upstream provenance and complete notices available. This is not a claim that every dependency is MIT licensed.

## Fully assembled local package

After native preparation, `.\tools\build.ps1` produces a complete local package under `dist/RustView-0.3.0`. It requires and includes the matching official 7-Zip source archive, and retains LibRaw's original source/binary ZIP. This local artifact is separate from the public bootstrap release. Redistribution of a fully assembled package requires accounting for all bundled upstream licenses and corresponding source/relink requirements, including static dependencies listed in ImageMagick's `NOTICE.txt`.

Both packaging scripts locate MSVC x64 redistributables through Visual Studio's `vswhere` or `VCToolsRedistDir`. If that workload is absent, pass `-RuntimeDirectory` pointing to a permitted `Microsoft.VC143.CRT` directory. Available upstream redist license text is copied alongside Microsoft attribution. No personal installation path is written into the package.

The scripts use system `cargo`, then an explicitly configured `CARGO_HOME`, or the development workspace's optional `.tools` Rust installation. They do not require the original developer's drive layout. Archives and generated user data are not placed in Git.

## Native replacement and sources

The versions, immutable URLs and archive/core digests are in `native/manifest.json`. Individual `native/*/SOURCE.txt`, licenses and ImageMagick's complete `NOTICE.txt` retain upstream source references. LibRaw is dynamically loaded from `native/LibRaw/libraw.dll`; an ABI-compatible x64 DLL can be substituted for a local build. 7-Zip, ImageMagick and ExifTool run as separate processes. A locally substituted engine changes the pinned runtime digest, so `Start-RustView.cmd` reports the difference; developers can review their build and launch `RustView.exe` directly. See [third-party notices](THIRD-PARTY-NOTICES.md).

## 中文说明

准备 Windows x64 的 Rust MSVC 工具链、Visual Studio C++ Build Tools 和 Windows SDK，然后在仓库根目录执行上面的三个准备／构建／测试命令。脚本兼容 Windows PowerShell 5.1 与 PowerShell 7；下载只在首次准备时需要联网，不安装软件，不修改系统 PATH、注册表或全局执行策略。

公开发行采用 **bootstrap 薄包**：解压后双击 `Start-RustView.cmd`，控制台会显示官方依赖的下载与 SHA-256 校验，完成后自动启动；之后可离线看图。请保留完整目录。依赖准备失败时控制台会显示原因，可重新运行启动器或 `tools/fetch-native.ps1`。不要仅复制单个 EXE。

`tools/package-public.ps1` 生成公开薄包；`tools/build.ps1` 则在依赖已准备后生成完整本地包，两者输出互不覆盖。完整本地包不作为本次 GitHub 发行资产。第三方引擎及其内部静态依赖并不统一使用 MIT 许可；原许可、精确来源与对应源码信息保留在 `native`，具体边界见第三方说明。公开仓库和发行压缩包均排除用户数据库、编辑记录、剪贴板缓存、工具链及相机样本。
