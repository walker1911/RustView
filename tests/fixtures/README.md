# Archive regression fixtures

These files contain only small generated images or upstream test text, never user data.

| Fixture | Contents | Password |
| --- | --- | --- |
| `plain.zip`, `plain.7z` | `image.png`, a generated 1×1 RGB PNG | none |
| `encrypted.zip` | The same PNG, ZipCrypto encryption | `fixture-password` |
| `aes.zip` | The same PNG, ZIP AES-256 encryption | `fixture-password` |
| `encrypted.7z` | The same PNG, encrypted 7z headers and data | `fixture-password` |
| `image.rar` | `test.tga`, a generated 1×1 grayscale TGA | none |
| `wildcards.zip` | Literal `*`, `@` and case-sensitive text entry names | none |
| `basic.rar` | Upstream libarchive text, directories and symlink | none |
| `test_read_format_rar4_encrypted_filenames.rar` | Four text entries, RAR4 encrypted filenames | `password` |
| `test_read_format_rar5_encrypted_filenames.rar` | Four text entries, RAR5 encrypted filenames | `password` |
| `test_read_format_rar5_solid_encrypted_filenames.rar` | Four text entries, solid RAR5 encrypted filenames | `password` |
| `test_read_format_rar5_encrypted.rar` | Text entries, non-encrypted filenames | `b.txt`: `password`; `d.txt`: `password2`; `a.txt`/`c.txt`: none |

Generated ZIP/7z fixtures use the bundled unmodified official 7-Zip 26.06 engine. Their passwords are public test constants.

The original RAR fixtures were decoded without modification from the corresponding `.rar.uu` files in [libarchive's test directory](https://github.com/libarchive/libarchive/tree/master/libarchive/test). Passwords and contents are documented by upstream in [test_read_format_rar_encryption.c](https://github.com/libarchive/libarchive/blob/master/libarchive/test/test_read_format_rar_encryption.c).

`basic.rar` comes from `test_read_format_rar.rar.uu`. `image.rar` is a derived test-only fixture: its stored `test.txt` record is renamed to `test.tga`, the 20-byte payload is replaced by the generated TGA, header/data checksums are recomputed, and other records are omitted. The application does not contain a RAR-writing implementation.

Upstream fixture redistribution notices are retained in `LIBARCHIVE-NOTICE.txt`.
