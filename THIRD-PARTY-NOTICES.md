# Third-party software

RustView's original application code is MIT licensed. Dependency versions are fixed in Cargo.lock. This project reuses libraries and native engines; it does not claim to be an official ACDSee build.

Full Rust dependency license and copyright texts are distributed in [licenses/RUST-DEPENDENCIES.txt](licenses/RUST-DEPENDENCIES.txt), with a [package and source-document inventory](licenses/RUST-DEPENDENCIES.json). The Windows graph contains 210 third-party packages, including 38 build/procedural-macro-only packages and three used in both build and runtime paths. The bundle preserves license alternatives, notices and embedded font licenses; it records the one supplemental standard CC0 text where hexf-parse omits a license document. The Rust 1.99.1 standard library's complete upstream attribution document is included in [licenses/RUST-STANDARD-LIBRARY.html](licenses/RUST-STANDARD-LIBRARY.html).

- egui / eframe / wgpu: https://github.com/emilk/egui and https://github.com/gfx-rs/wgpu — MIT / Apache-2.0.
- image and image-webp: https://github.com/image-rs — MIT / Apache-2.0; see the locked packages for individual notices.
- gif, png and tiff: https://github.com/image-rs — MIT / Apache-2.0; animation metadata, actual output bit depths and multipage TIFF decoding.
- rusqlite: https://github.com/rusqlite/rusqlite — MIT. The bundled SQLite engine is public domain: https://sqlite.org/copyright.html .
- windows / windows-sys: https://github.com/microsoft/windows-rs — MIT / Apache-2.0; keyboard, clipboard, printing and desktop wallpaper interfaces.
- moxcms: https://github.com/awxkee/moxcms — BSD-3-Clause / Apache-2.0.
- LibRaw 0.22.3: https://www.libraw.org/ — dual LGPL-2.1 / CDDL-1.0. Provenance, copyright, licenses and download digests are in native/LibRaw and native/manifest.json. The official Windows ZIP includes the source and is retained after preparation.
- ImageMagick 7.1.2-35 portable Q16-HDRI x64: https://github.com/ImageMagick/ImageMagick — ImageMagick License and bundled codec licenses. Upstream source links, digests, LICENSE and full bundled-code NOTICE are retained in native/ImageMagick and native/manifest.json. The public bootstrap does not redistribute this native executable. Invoked as a separate process with an image-only resource policy for HEIC/HEIF, AVIF/JXL and high-depth processing.
- ExifTool 13.62 portable x64: https://exiftool.org/ — same terms as Perl (Artistic License or GPL). Source/provenance information is retained in native/ExifTool; preparation downloads the unmodified launcher, source modules and runtime licenses together, including exiftool_files. Invoked as a separate process for EXIF/IPTC/XMP interoperability.
- 7-Zip 26.06 x64: https://www.7-zip.org/ — LGPL and other components, including the unRAR restriction. See native/7zip/License.txt and SOURCE.txt; the matching source archive is fetched into the ignored local cache and included in a fully assembled local package.
- Other Rust dependencies include rfd, serde, serde_json, libloading, tempfile, natord, arboard, trash and windows-sys. Their exact versions, license metadata and full license texts are in the distributed Rust dependency inventory above.
- When present, app-local Microsoft VC++ runtime DLLs originate from the installed Microsoft Visual Studio Build Tools redistributable. They are Microsoft components, not covered by this project's MIT license.

Application architecture was informed by public Rust viewers including ViewSkater and cbz-tools-viewer. No ACDSee executable code is linked into this application; the shortcut catalog records behavior from the user's supplied reference program.


The public source and bootstrap ZIP exclude native engine binaries; the bootstrap preparation script downloads pinned upstream distributions after SHA-256 verification. The complete offline ZIP includes those engines and their original notices, with corresponding source and rebuild materials on the same Release. See [native source guide](docs/NATIVE-SOURCES.md), [BUILDING.md](BUILDING.md) and [publication inventory](docs/PUBLICATION-AUDIT.md). Application MIT licensing does not replace the licenses or redistribution requirements of the engines and their bundled dependencies.
