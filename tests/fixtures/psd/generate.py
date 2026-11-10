"""Generate tiny original PSD/PSB regression images, without external packages.

All image data is synthetic. No Photoshop/user artwork is incorporated.
Layout follows Adobe's public Photoshop file format specification.
"""
from pathlib import Path
import struct


HERE = Path(__file__).resolve().parent


def u16(value):
    return struct.pack(">H", value)


def u32(value):
    return struct.pack(">I", value)


def resource(kind, value):
    return b"8BIM" + u16(kind) + b"\0\0" + u32(len(value)) + value + b"\0" * (len(value) % 2)


def image(name, planes, *, width=2, height=1, depth=8, mode=3, layered=False, transparent=False, version=1, resources=b"", rle=False):
    large = version == 2
    size = (lambda n: struct.pack(">Q", n)) if large else u32
    header = b"8BPS" + u16(version) + b"\0" * 6 + u16(len(planes))
    header += u32(height) + u32(width) + u16(depth) + u16(mode)
    layer_mask = b""
    if layered:
        # The actual layer is blue; the saved composite is orange. Selecting
        # a layer or recompositing it gives a deliberately different image.
        channels = [bytes([0, 0]), bytes([0, 0]), bytes([255, 255])]
        record = struct.pack(">iiiiH", 0, 0, 1, 2, len(channels))
        record += b"".join(u16(n) + size(len(channel) + 2) for n, channel in enumerate(channels))
        extra = u32(0) + u32(0) + b"\x04blue\0\0\0"
        record += b"8BIMnorm\xff\0\0\0" + u32(len(extra)) + extra
        layers = struct.pack(">h", -1 if transparent else 1) + record + b"".join(u16(0) + channel for channel in channels)
        layers += b"\0" * (len(layers) % 2)
        layer_mask = size(len(layers)) + layers + u32(0)
    pixels = b"".join(bytes(plane) if depth == 8 else b"".join(u16(v) for v in plane) for plane in planes)
    data = header + u32(0) + u32(len(resources)) + resources + size(len(layer_mask)) + layer_mask
    if rle:
        # Two samples per row: a PackBits literal packet, one row per plane.
        rows = [bytes([width - 1]) + bytes(plane) for plane in planes]
        pixels = b"".join(u16(len(row)) for row in rows) + b"".join(rows)
    (HERE / name).write_bytes(data + u16(1 if rle else 0) + pixels)


if __name__ == "__main__":
    image("rgb8.psd", [[255, 0], [0, 255], [0, 0]])
    # PSD's merged RGB pixels use Photoshop's white-blended alpha convention.
    image("rgba8.psd", [[255, 0], [127, 255], [127, 0], [128, 255]], layered=True, transparent=True)
    image("rgb-rle.psd", [[255, 0], [0, 255], [0, 0]], rle=True)
    image("rgb16.psd", [[16384, 65535], [32768, 0], [49152, 0]], depth=16)
    # Photoshop stores inverted CMYK channels: white then solid cyan.
    image("cmyk8.psd", [[255, 0], [255, 255], [255, 255], [255, 255]], mode=4)
    image("layered.psd", [[240, 240], [120, 120], [30, 30]], layered=True)
    image("layered.psb", [[240, 240], [120, 120], [30, 30]], layered=True, version=2)
    # Embed the exact bundled sRGB ICC profile as Photoshop resource 1039.
    profile = (HERE.parents[2] / "native" / "ImageMagick" / "sRGB.icc").read_bytes()
    image("rgb-icc.psd", [[90, 90], [120, 120], [180, 180]], resources=resource(1039, profile))
