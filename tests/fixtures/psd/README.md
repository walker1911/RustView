# Synthetic Photoshop fixtures

All eight images are original, generated 2 × 1 pixel files. They contain no private or third-party artwork and are covered by the repository's MIT license. Regenerate them with `python tests/fixtures/psd/generate.py` from the repository root after fetching the bundled native engines.

| File | Purpose | Expected displayed RGBA pixels |
| --- | --- | --- |
| `rgb8.psd` | Raw 8-bit RGB composite | `(255,0,0,255)`, `(0,255,0,255)` |
| `rgb-rle.psd` | PackBits/RLE RGB composite | Same as `rgb8.psd` |
| `rgba8.psd` | Signed negative layer count marks composite transparency; Photoshop white-blended RGB | `(255,0,0,128)`, `(0,255,0,255)` |
| `rgb16.psd` | 16-bit RGB composite; display conversion rounds to RGBA8 | `(64,128,191,255)`, `(255,0,0,255)` |
| `cmyk8.psd` | Inverted Photoshop CMYK; no embedded profile | White, cyan |
| `rgb-icc.psd` | Embedded bundled sRGB ICC as image resource 1039 | `(90,120,180,255)` twice |
| `layered.psd` | RGB composite is orange; its independent layer is blue | `(240,120,30,255)` twice |
| `layered.psb` | Same content using PSB version 2, 64-bit section/channel sizes | Same orange composite |

The layered cases deliberately distinguish selecting the saved composite from selecting or reconstructing a layer. The RGBA case follows Photoshop's composite alpha convention; extra channels alone do not establish transparency.

The fixture layout follows [Adobe's file format specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/). The ICC bytes come from the already bundled official ImageMagick `sRGB.icc`; they are a standard color profile, not artwork. Runtime bounds and exact expected pixels are asserted in `src/precision.rs`. The test suite does not use a private Photoshop document or require Photoshop.
