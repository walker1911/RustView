# Public-source inventory

Prepared 2026-11-10. This records the publication inputs and provenance; it does not describe an affiliation with ACD Systems or other vendors.

## Application and compatibility data

The application source is original RustView project code under the root MIT license. Runtime image, archive and metadata engines retain their separate upstream licenses.

The public `docs/retained-command-audit.csv` is a project-authored functional interface map. Its short labels describe RustView dispatch behavior. It preserves the numeric compatibility identifiers and command states: 257 rows, 206 retained commands, 32 excluded commands and 19 inactive/unconfirmed commands. It does not contain extracted menus, copied help descriptions, resource hierarchies, binary offsets, or images from the reference application.

The complete local resource inventories (`shortcut-catalog.csv` and `command-map.md`) are research inputs and are excluded from the public source tree. The unabridged resource-derived audit table is also excluded; the compact public table replaces it at the same compile-time path. The allowed command ID set was checked to be identical before and after the replacement.

## Test data allowlist

The following small files are suitable inputs for the public source tree, with the provenance documents alongside them. They contain synthetic pixels or upstream test text, not user photographs.

- Project-generated pixels and archives: `image.png`, `test.webp`, `plain.zip`, `plain.7z`, `encrypted.zip`, `aes.zip`, `encrypted.7z`, `wildcards.zip`.
- Project-generated animation data: `animation-background.gif`, `animation-previous.gif`, `animation-blend.webp`. Keep `animation-fixtures.txt` describing their generation and expected disposal/timing behavior.
- Upstream libarchive test archives: `basic.rar`, `test_read_format_rar4_encrypted_filenames.rar`, `test_read_format_rar5_encrypted_filenames.rar`, `test_read_format_rar5_solid_encrypted_filenames.rar`, `test_read_format_rar5_encrypted.rar`.
- `image.rar` is a test-only derivative of the upstream stored RAR fixture: a generated TGA replaces a stored text payload; header/data checksums are recomputed. This does not add RAR compression or writing to the application.
- Keep `tests/fixtures/README.md` and the expanded `LIBARCHIVE-NOTICE.txt`. The fixture passwords are intentionally public test constants.
- Keep `native/LibRaw/testdata/synthetic.dng` and its README. It is the project's 32x24 synthetic CFA input, required by compile-time tests, not a real camera photograph.

The upstream fixture notices were compared with the license headers of:

- [libarchive RAR encryption tests](https://github.com/libarchive/libarchive/blob/master/libarchive/test/test_read_format_rar_encryption.c)
- [libarchive basic RAR tests](https://github.com/libarchive/libarchive/blob/master/libarchive/test/test_read_format_rar.c)
- [libarchive project license inventory](https://github.com/libarchive/libarchive/blob/master/COPYING)

The basic RAR attribution additionally includes the named copyright lines from its associated upstream test. Retain both that attribution and the encryption test notice.

## Native engine inventory

Keep source/provenance notices, licenses, and RustView's own image-only `policy.xml` in source control. Obtain runtime binaries directly from the pinned official downloads through the dependency setup script.

- 7-Zip: `native/7zip/License.txt`, `SOURCE.txt`. The matching source archive is explicitly identified, including its SHA-256.
- LibRaw: `COPYRIGHT`, `LICENSE.CDDL`, `LICENSE.LGPL`, `SOURCE.txt`, `UPSTREAM-README.md`, plus the synthetic test fixture above. The original official Windows ZIP contains the corresponding C/C++ sources as well as runtime binaries; it is not an application MIT asset.
- ImageMagick: `LICENSE.txt`, complete `NOTICE.txt`, `SOURCE.txt`, and the project policy. The portable executable's NOTICE identifies multiple separately licensed dependencies, including libde265 1.1.3 / libheif 1.23.7 under LGPLv3, and contains exact upstream source commits. Preserve the complete NOTICE, not only the ImageMagick license.
- ExifTool: `SOURCE.txt`; the official runtime download must retain `exiftool_files`, its licenses, Perl modules, and runtime notices.

The source repository and bootstrap ZIP use official-download dependency setup. The additional complete offline ZIP includes the native binaries and original notices; download links for matching source and Windows rebuild materials are recorded in [NATIVE-SOURCES.md](NATIVE-SOURCES.md). Redistributors must preserve the applicable source/build/relink materials and must not describe the whole bundle as MIT-licensed. Running an external native engine does not change RustView's own source license.

## Files outside the source allowlist

Do not publish local toolchains/caches, build output, runtime data, databases/WALs, desktop wallpaper backups, clipboard exports, generated logs, original reference executables or patches, private resource inventories, or downloaded real-camera photographs. Those are outside the public source tree. Public validation summaries should describe reproducible scope and limitations without embedding local paths or user-specific environment snapshots.

A text scan of application source, build files, tests, and public-facing docs found no private-key markers or real access-token patterns. Password-like values found in tests are documented fixture constants; generic sample drive paths are test data. This is an inventory observation, not a claim that source review replaces GitHub secret scanning.
