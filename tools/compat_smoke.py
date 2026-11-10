"""Run bounded, sequential compatibility checks against a real RustView release worker."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import threading
import time
import zlib

from animation_smoke import FLAGS, private_tree_bytes
from smoke_test import stop_own_tree, write_png


PROJECT = Path(__file__).resolve().parent.parent
MIB = 1024 * 1024
CAMERAS = (
    ("Nikon-Z6.NEF", (6048, 4024), (6064, 4040)),
    ("Sony-a7III.ARW", (1616, 1080), (6024, 4024)),
    ("Canon-EOS-R.CR3", (6720, 4480), (6742, 4498)),
    ("Apple-iPhone-12-Pro.DNG", (3024, 4032), (3024, 4032)),
    ("libheif-example.heic", (1280, 854), (1280, 854)),
)


def digest(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as source:
        for chunk in iter(lambda: source.read(MIB), b""):
            value.update(chunk)
    return value.hexdigest()


def load(path, full_raw=False):
    return {"operation": "Load", "path": str(path), "entry": None,
            "password": None, "full_raw": full_raw}


def render(source, *, save=None, edits=None, format_name="Png", depth=16, animation="CurrentFrame"):
    return {"operation": "Render", "source": source, "edits": edits or [],
            "settings": {"format": format_name, "depth": depth, "quality": 95,
                         "preserve_metadata": True, "animation": animation},
            "raw": {}, "save": str(save) if save is not None else None}


def run_request(exe, request, timeout):
    """Read stdout concurrently so memory sampling never blocks on a full pipe."""
    started = time.monotonic()
    process = subprocess.Popen([str(exe), "--request-probe"], stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, creationflags=FLAGS)
    done = threading.Event()
    result = {}
    samples = []

    def communicate():
        try:
            result["streams"] = process.communicate(json.dumps(request, ensure_ascii=False).encode("utf-8"))
        except BaseException as error:
            result["exception"] = error
        finally:
            done.set()

    reader = threading.Thread(target=communicate, daemon=True)
    reader.start()
    timed_out = False
    try:
        while True:
            sampled = private_tree_bytes(process.pid)
            if sampled is not None:
                samples.append(sampled)
            if done.wait(0.1):
                break
            if time.monotonic() - started >= timeout:
                timed_out = True
                print(json.dumps({"event": "timeout", "pid": process.pid, "limit_seconds": timeout}), flush=True)
                stop_own_tree(process)
                reader.join(timeout=10)
                break
    except BaseException:
        stop_own_tree(process)
        reader.join(timeout=10)
        raise
    elapsed = time.monotonic() - started
    memory = {"metric": "probe + descendants private committed bytes",
              "interval_ms": 100, "sample_count": len(samples),
              "peak_bytes": max(samples) if samples else None}
    details = {"elapsed_seconds": round(elapsed, 3), "memory": memory, "exit_code": process.poll()}
    if timed_out:
        return {**details, "status": "timeout", "reason": f"Exceeded {timeout:g} seconds; owned process tree terminated"}
    if "exception" in result:
        return {**details, "status": "fail", "reason": str(result["exception"])}
    stdout, stderr = result.get("streams", (b"", b""))
    try:
        reply = json.loads(stdout.decode("utf-8-sig"))
    except (ValueError, UnicodeError):
        return {**details, "status": "fail", "reason": "Probe did not return valid JSON",
                "stderr": stderr.decode("utf-8", errors="replace")[:2000]}
    details["reply"] = {key: value for key, value in reply.items()
                        if key not in ("metadata", "editing_record", "entries")}
    if process.returncode or reply.get("error"):
        return {**details, "status": "fail", "reason": reply.get("error") or "Worker returned a failure status",
                "stderr": stderr.decode("utf-8", errors="replace")[:2000]}
    return {**details, "status": "pass", "_full_reply": reply}


def expect_size(reply, expected):
    actual = (reply.get("width"), reply.get("height"))
    if actual != tuple(expected):
        raise ValueError(f"Decoded size {actual} != expected {tuple(expected)}")


def png_info(path):
    """Inspect PNG structure/ICC with bounded chunks; never decode a full photo in Python."""
    info = {}
    idat = hashlib.sha256()
    with Path(path).open("rb") as source:
        if source.read(8) != b"\x89PNG\r\n\x1a\n":
            raise ValueError("Output is not PNG")
        while True:
            header = source.read(8)
            if len(header) != 8:
                raise ValueError("Truncated PNG chunk header")
            length, kind = struct.unpack(">I4s", header)
            if length > 512 * MIB:
                raise ValueError("PNG chunk exceeds the file budget")
            crc = zlib.crc32(kind)
            saved = bytearray() if kind in (b"IHDR", b"iCCP") else None
            if saved is not None and length > 8 * MIB:
                raise ValueError("PNG metadata chunk exceeds the metadata budget")
            remaining = length
            while remaining:
                data = source.read(min(remaining, MIB))
                if not data:
                    raise ValueError("Truncated PNG payload")
                remaining -= len(data)
                crc = zlib.crc32(data, crc)
                if kind == b"IDAT":
                    idat.update(data)
                if saved is not None:
                    saved.extend(data)
            trailer = source.read(4)
            if len(trailer) != 4 or struct.unpack(">I", trailer)[0] != crc & 0xffffffff:
                raise ValueError("PNG CRC mismatch")
            if kind == b"IHDR":
                width, height, bits, color, compression, filtering, interlace = struct.unpack(">IIBBBBB", saved)
                info.update(width=width, height=height, bit_depth=bits, color_type=color,
                            compression=compression, filter=filtering, interlace=interlace)
            elif kind == b"iCCP":
                name, separator, payload = bytes(saved).partition(b"\0")
                if not separator or len(payload) < 2 or payload[0] != 0:
                    raise ValueError("Invalid compressed ICC header")
                decoder = zlib.decompressobj()
                profile = decoder.decompress(payload[1:], 8 * MIB + 1)
                if len(profile) > 8 * MIB or not decoder.eof:
                    raise ValueError("ICC profile is too large or truncated")
                if len(profile) < 132 or profile[36:40] != b"acsp" or int.from_bytes(profile[:4], "big") != len(profile):
                    raise ValueError("PNG ICC profile has an invalid ICC header")
                info["icc"] = {"name": name.decode("latin-1"), "bytes": len(profile),
                               "color_space": profile[16:20].decode("ascii", errors="replace"),
                               "sha256": hashlib.sha256(profile).hexdigest()}
            elif kind == b"IEND":
                break
    info["idat_sha256"] = idat.hexdigest()
    return info


def check_png(reply, destination, expected):
    summary = reply.get("rendered") or {}
    if (summary.get("width"), summary.get("height")) != tuple(expected):
        raise ValueError("Render summary dimensions differ from the reference sample")
    info = png_info(destination)
    if (info["width"], info["height"]) != tuple(expected) or info["bit_depth"] != 16:
        raise ValueError(f"Expected {tuple(expected)}/PNG16, got {(info['width'], info['height'])}/PNG{info['bit_depth']}; ICC present={bool(info.get('icc'))}")
    if not info.get("icc"):
        raise ValueError("High-depth output did not include an ICC profile")
    return {"output": str(destination), "png": info, "sha256": digest(destination)}


def subblocks(data, offset):
    output = bytearray()
    while True:
        if offset >= len(data):
            raise ValueError("Truncated GIF sub-block")
        size = data[offset]
        offset += 1
        if not size:
            return bytes(output), offset
        if offset + size > len(data):
            raise ValueError("Truncated GIF extension payload")
        output.extend(data[offset:offset + size])
        offset += size


def animation_info(path):
    if Path(path).stat().st_size > 16 * MIB:
        raise ValueError("Animation fixture output exceeds the parser budget")
    data = Path(path).read_bytes()
    if data[:6] in (b"GIF87a", b"GIF89a"):
        width, height = struct.unpack_from("<HH", data, 6)
        packed = data[10]
        offset = 13 + (3 * (2 << (packed & 7)) if packed & 0x80 else 0)
        delays, plays, pending = [], None, 10
        while offset < len(data):
            marker = data[offset]
            offset += 1
            if marker == 0x3b:
                break
            if marker == 0x21:
                label = data[offset]
                offset += 1
                payload, offset = subblocks(data, offset)
                if label == 0xf9:
                    pending = max(10, int.from_bytes(payload[1:3], "little") * 10)
                elif label == 0xff and payload.startswith((b"NETSCAPE2.0", b"ANIMEXTS1.0")):
                    loops = int.from_bytes(payload[12:14], "little")
                    plays = 0 if loops == 0 else loops + 1
            elif marker == 0x2c:
                packed = data[offset + 8]
                offset += 9 + (3 * (2 << (packed & 7)) if packed & 0x80 else 0)
                offset += 1  # minimum LZW code size
                _, offset = subblocks(data, offset)
                delays.append(pending)
                pending = 10
            else:
                raise ValueError("Unexpected GIF block marker")
        return {"format": "GIF", "width": width, "height": height,
                "frames": len(delays), "delays_ms": delays, "total_plays": plays}
    if data[:4] != b"RIFF" or data[8:12] != b"WEBP":
        raise ValueError("Output is neither GIF nor WebP")
    if int.from_bytes(data[4:8], "little") + 8 != len(data):
        raise ValueError("WebP RIFF length mismatch")
    info = {"format": "WebP", "frames": 0, "delays_ms": [], "total_plays": None}
    offset = 12
    while offset + 8 <= len(data):
        kind, size = struct.unpack_from("<4sI", data, offset)
        payload = data[offset + 8:offset + 8 + size]
        if len(payload) != size:
            raise ValueError("Truncated WebP chunk")
        if kind == b"VP8X":
            info.update(width=int.from_bytes(payload[4:7], "little") + 1,
                        height=int.from_bytes(payload[7:10], "little") + 1)
        elif kind == b"ANIM":
            info["total_plays"] = int.from_bytes(payload[4:6], "little")
        elif kind == b"ANMF":
            info["frames"] += 1
            info["delays_ms"].append(max(10, int.from_bytes(payload[12:15], "little")))
        offset += 8 + size + (size & 1)
    return info


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, default=PROJECT / "test-output" / "compat-v03")
    parser.add_argument("--timeout", type=float, default=150)
    parser.add_argument("--only", action="append", help="Run matching case names only; may be repeated")
    args = parser.parse_args()
    if not 0 < args.timeout <= 150:
        parser.error("Each request timeout must be in (0, 150] seconds")
    exe, work, output = args.exe.resolve(), args.work_dir.resolve(), args.output.resolve()
    if not exe.is_file():
        parser.error("Executable does not exist")
    work.mkdir(parents=True, exist_ok=True)
    output.parent.mkdir(parents=True, exist_ok=True)
    fixtures = PROJECT / "tests" / "fixtures"
    samples = PROJECT / "test-output" / "modern-raw-samples"
    report = {"status": "running", "executable": str(exe), "executable_sha256": digest(exe), "timeout_seconds": args.timeout,
              "scope": "Sequential release-worker requests; sampled tree private committed memory, not GUI memory or long-run stress",
              "cases": []}
    source_hashes = {name: digest(samples / name) for name, _, _ in CAMERAS if (samples / name).is_file()}

    def save_report():
        report["passed"] = sum(c["status"] == "pass" for c in report["cases"])
        report["failed"] = sum(c["status"] != "pass" for c in report["cases"])
        temporary = output.with_suffix(output.suffix + ".tmp")
        temporary.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        temporary.replace(output)

    def case(name, request, validate):
        if args.only and not any(part in name for part in args.only):
            return
        print(json.dumps({"event": "start", "case": name}, ensure_ascii=False), flush=True)
        try:
            result = run_request(exe, request, args.timeout)
        except Exception as error:
            result = {"status": "fail", "reason": str(error), "elapsed_seconds": None,
                      "memory": {"sample_count": 0, "peak_bytes": None}, "exit_code": None}
        if request.get("save"):
            result["output"] = request["save"]
        reply = result.pop("_full_reply", None)
        if result["status"] == "pass":
            try:
                result["checks"] = validate(reply) or {}
            except Exception as error:
                result.update(status="fail", reason=str(error))
        result["case"] = name
        report["cases"].append(result)
        save_report()
        print(json.dumps({"event": "done", "case": name, "status": result["status"],
                          "seconds": result["elapsed_seconds"], "peak_bytes": result["memory"]["peak_bytes"],
                          "reason": result.get("reason")}, ensure_ascii=False), flush=True)

    for name, preview_size, full_size in CAMERAS:
        source = samples / name
        case("load_" + name, load(source), lambda reply, size=preview_size: expect_size(reply, size))
        destination = work / (name + "-rgb16.png")
        case("render16_" + name, render(load(source, True), save=destination),
             lambda reply, path=destination, size=full_size: check_png(reply, path, size))

    preview_source = work / "preview-source.png"
    write_png(preview_source, 96, 64, transparent=True)
    previews = (
        ("preview_render_resize", [{"Resize": {"width": 320, "height": 180}}], 128, None, (128, 72), (320, 180)),
        ("preview_render_rotate", [{"Rotate90": {"turns": 1}}], 48, None, (32, 48), (64, 96)),
        ("preview_render_region", [{"Resize": {"width": 200, "height": 150}}], 64, [20, 10, 80, 60], (64, 48), (200, 150)),
    )
    for name, edits, side, region, expected, source_size in previews:
        request = {"operation": "Preview", "source": render(load(preview_source), edits=edits),
                   "max_side": side, "region": region}

        def validate_preview(reply, expected=expected, source_size=source_size, region=region):
            expect_size(reply, expected)
            actual = (reply.get("source_width"), reply.get("source_height"))
            if actual != source_size:
                raise ValueError(f"Original rendered dimensions {actual} != {source_size}")
            return {"output_size": list(expected), "rendered_size": list(source_size), "region": region}

        case(name, request, validate_preview)

    values = {"title": "兼容回归标题\nsecond line", "description": "自建夹具说明", "keywords": ["ACDSee", "中文"],
              "iptc_keywords": ["IPTC", "标准关键词"], "rating": 4, "label": "Red", "creator": "测试摄影师",
              "copyright": "RustView fixture 2026", "datetime_original": "2026:11:10 18:23:04"}
    for suffix, original, destination_type in (("png", preview_source, "Embedded"),
                                               ("raw", PROJECT / "native" / "LibRaw" / "testdata" / "synthetic.dng", "Auto")):
        path = work / ("metadata-中文.png" if suffix == "png" else "metadata-RAW.dng")
        shutil.copyfile(original, path)
        before = digest(path)

        def validate_write(reply, path=path, before=before, suffix=suffix):
            written = reply.get("metadata_write") or {}
            target = Path(written.get("destination", ""))
            if not target.is_file():
                raise ValueError("Worker did not return an existing metadata destination")
            if suffix == "raw":
                if target.suffix.lower() != ".xmp" or digest(path) != before:
                    raise ValueError("RAW was changed or metadata did not use an XMP sidecar")
            else:
                backup = written.get("backup")
                if not backup or digest(backup) != before:
                    raise ValueError("First original backup does not match the source fixture")
                if png_info(path)["idat_sha256"] != png_info(preview_source)["idat_sha256"]:
                    raise ValueError("Metadata update changed compressed image pixels")
            return {"destination": str(target), "raw_unchanged": suffix == "raw", "backup": written.get("backup")}

        case("metadata_write_" + suffix, {"operation": "MetadataWrite", "path": str(path), "values": values,
                                          "destination": destination_type}, validate_write)

        def validate_read(reply):
            metadata = reply.get("metadata") or {}
            if metadata.get("values") != values:
                raise ValueError("Real metadata read did not match the fields written by the worker")
            return {"values": metadata["values"], "sidecar": metadata.get("sidecar"),
                    "field_count": len(metadata.get("fields", {}))}

        case("metadata_read_" + suffix, {"operation": "MetadataRead", "source": load(path)}, validate_read)

    odd = work / "odd-delay.webp"
    data = bytearray((fixtures / "animation-blend.webp").read_bytes())
    offset, index = 12, 0
    while offset + 8 <= len(data):
        size = int.from_bytes(data[offset + 4:offset + 8], "little")
        if data[offset:offset + 4] == b"ANMF":
            data[offset + 20:offset + 23] = (13, 27, 10)[index].to_bytes(3, "little")
            index += 1
        offset += 8 + size + (size & 1)
    odd.write_bytes(data)
    for suffix, source, format_name in (("gif", fixtures / "animation-previous.gif", "Gif"), ("webp", odd, "WebP")):
        destination = work / ("animation-export." + suffix)
        expected_animation = animation_info(source)
        expected_animation.update(width=8, height=4)

        def validate_animation(reply, destination=destination, expected=expected_animation):
            actual = animation_info(destination)
            if actual != expected:
                raise ValueError(f"Exported animation structure changed: {actual} != {expected}")
            summary = reply.get("rendered") or {}
            if summary.get("frames") != actual["frames"]:
                raise ValueError("Render frame count differs from the encoded file")
            return {"output": str(destination), "animation": actual, "sha256": digest(destination)}

        case("animation_export_" + suffix, render(load(source), save=destination,
             edits=[{"Resize": {"width": 8, "height": 4}}], format_name=format_name,
             depth=8, animation="AllFrames"), validate_animation)

    report["source_hashes_unchanged"] = all(digest(samples / name) == before for name, before in source_hashes.items())
    report["source_sha256"] = source_hashes
    report["executable_unchanged"] = digest(exe) == report["executable_sha256"]
    report["status"] = "pass" if report["cases"] and all(c["status"] == "pass" for c in report["cases"]) and report["source_hashes_unchanged"] and report["executable_unchanged"] else "fail"
    save_report()
    print(json.dumps({"passed": report["passed"], "failed": report["failed"],
                      "source_unchanged": report["source_hashes_unchanged"],
                      "executable_unchanged": report["executable_unchanged"], "report": str(output)}, ensure_ascii=False), flush=True)
    return report["status"] != "pass"


if __name__ == "__main__":
    raise SystemExit(main())
