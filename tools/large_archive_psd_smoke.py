"""Sequential real-worker checks for >512 MiB PSD entries in ZIP/RAR/7z.

All large artwork is synthetic and generated under test-output, never checked in.
RAR5 stored blocks follow https://www.rarlab.com/technote.htm; no RAR writer is
installed or required. See tests/fixtures/psd/LARGE-ARCHIVES.md for provenance.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import threading
import time
import zipfile
import zlib

from compat_smoke import FLAGS, MIB, digest, load, png_info, private_tree_bytes, render, run_request
from smoke_test import stop_own_tree


PROJECT = Path(__file__).resolve().parent.parent
ENTRY = "large-composite.psd"
PASSWORD = "fixture-password"


def write_large_psd(path):
    """One large raw black layer, a deliberately different orange 2x1 composite."""
    plane = 14000 * 13000
    extra = struct.pack(">II", 0, 0) + b"\x05large\0\0"
    record = struct.pack(">iiiiH", 0, 0, 13000, 14000, 3)
    record += b"".join(struct.pack(">hI", n, plane + 2) for n in range(3))
    record += b"8BIMnorm\xff\0\0\0" + struct.pack(">I", len(extra)) + extra
    layer_size = 2 + len(record) + 3 * (plane + 2)
    assert layer_size % 2 == 0
    with path.open("wb") as output:
        output.write(b"8BPS\0\x01" + b"\0" * 6 + struct.pack(">HIIHH", 3, 1, 2, 8, 3))
        output.write(struct.pack(">IIIIh", 0, 0, 4 + layer_size + 4, layer_size, 1))
        output.write(record)
        for _ in range(3):
            output.write(b"\0\0")
            output.seek(plane, 1)
        output.write(struct.pack(">I", 0) + b"\0\0\xf0\xf0\x78\x78\x1e\x1e")
    assert path.stat().st_size > 512 * MIB


def vint(value):
    result = bytearray()
    while value >= 128:
        result.append((value & 127) | 128)
        value >>= 7
    result.append(value)
    return bytes(result)


def rar_block(body):
    data = vint(len(body)) + body
    return struct.pack("<I", zlib.crc32(data)) + data


def write_stored_rar(source, target):
    crc = 0
    with source.open("rb") as input_file:
        for data in iter(lambda: input_file.read(MIB), b""):
            crc = zlib.crc32(data, crc)
    size, name = source.stat().st_size, ENTRY.encode("utf-8")
    # File block with a data area, CRC32, Windows archive attribute, method=store.
    body = vint(2) + vint(2) + vint(size) + vint(4) + vint(size) + vint(0x20)
    body += struct.pack("<I", crc) + vint(0) + vint(0) + vint(len(name)) + name
    with target.open("wb") as output, source.open("rb") as input_file:
        output.write(b"Rar!\x1a\x07\x01\0" + rar_block(vint(1) + vint(0) + vint(0)))
        output.write(rar_block(body))
        shutil.copyfileobj(input_file, output, MIB)
        output.write(rar_block(vint(5) + vint(0) + vint(0)))


def seven_zip(arguments, work):
    command = [str(PROJECT / "native/7zip/7z.exe"), *arguments]
    result = subprocess.run(command, cwd=work, stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            creationflags=FLAGS, timeout=120)
    if result.returncode:
        raise ValueError("Fixture 7-Zip failed: " + result.stderr.decode("utf-8", "replace")[:2000])


def prepare(work):
    source = work / ENTRY
    write_large_psd(source)
    archives = {}
    for kind in ["zip", "7z"]:
        for encrypted in [False, True]:
            label = kind + ("-aes" if encrypted else "")
            archive = work / (label + "." + kind)
            if archive.exists():
                archive.unlink()
            arguments = ["a", "-t" + kind, "-mx=1", "-mmt=1"]
            if kind == "7z":
                arguments += ["-md=1m"]
            if encrypted:
                arguments += ["-p" + PASSWORD, "-mhe=on" if kind == "7z" else "-mem=AES256"]
            seven_zip([*arguments, str(archive), ENTRY], work)
            seven_zip(["t", "-p" + PASSWORD, str(archive)], work)
            archives[label] = archive
    archives["rar"] = work / "stored.rar"
    write_stored_rar(source, archives["rar"])
    seven_zip(["t", str(archives["rar"])], work)
    # Content is deliberately PSD, but a non-PSD entry name must keep 512 MiB.
    ordinary = work / "ordinary-limit.zip"
    with zipfile.ZipFile(ordinary, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=1) as output:
        output.write(source, "oversized.png")
    return source, archives, ordinary


def orange_png(path):
    if path.stat().st_size > MIB:
        return False
    info = png_info(path)
    if (info["width"], info["height"], info["bit_depth"]) != (2, 1, 16):
        return False
    if info["color_type"] not in (2, 6) or info["interlace"]:
        return False
    data = path.read_bytes()
    offset, compressed = 8, bytearray()
    while offset < len(data):
        length, kind = struct.unpack_from(">I4s", data, offset)
        if kind == b"IDAT":
            compressed.extend(data[offset + 8:offset + 8 + length])
        offset += length + 12
    decoder = zlib.decompressobj()
    raw = decoder.decompress(compressed, 100)
    channels = 3 if info["color_type"] == 2 else 4
    bpp = channels * 2
    if not decoder.eof or len(raw) != 1 + 2 * bpp or raw[0] > 4:
        return False
    row = bytearray(raw[1:])
    for n in range(len(row)):
        left = row[n - bpp] if n >= bpp else 0
        if raw[0] in (1, 4):
            row[n] = (row[n] + left) & 255
        elif raw[0] == 3:
            row[n] = (row[n] + left // 2) & 255
    samples = struct.unpack(">" + "H" * (2 * channels), row)
    expected = (240, 120, 30) + ((255,) if channels == 4 else ())
    return all(abs(value / 257 - expected[n % channels]) < 1 for n, value in enumerate(samples))


def cancel_request(exe, request, temporary):
    started = time.monotonic()
    process = subprocess.Popen([str(exe), "--request-probe", "--cancel-after-ms", "250"],
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, creationflags=FLAGS)
    done, streams, samples, partial = threading.Event(), [], [], 0

    def communicate():
        try:
            streams.append(process.communicate(json.dumps(request).encode("utf-8")))
        finally:
            done.set()

    reader = threading.Thread(target=communicate, daemon=True)
    reader.start()
    while not done.wait(0.02):
        memory = private_tree_bytes(process.pid)
        if memory is not None:
            samples.append(memory)
        for path in temporary.rglob("input.psd"):
            try:
                partial = max(partial, path.stat().st_size)
            except FileNotFoundError:
                pass
        if time.monotonic() - started > 20:
            stop_own_tree(process)
            reader.join(timeout=10)
            break
    reader.join(timeout=10)
    stdout, stderr = streams[0] if streams else (b"", b"No probe output")
    try:
        reply = json.loads(stdout.decode("utf-8-sig"))
    except (ValueError, UnicodeError):
        reply = {"error": stderr.decode("utf-8", "replace")[:2000]}
    return {"elapsed_seconds": round(time.monotonic() - started, 3),
            "exit_code": process.poll(), "reply": reply, "status": "fail",
            "observed_partial_bytes": partial,
            "memory": {"metric": "probe + descendants private committed bytes", "interval_ms": 20,
                       "sample_count": len(samples), "peak_bytes": max(samples) if samples else None}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    if not args.prepare_only and (args.exe is None or not args.exe.is_file()):
        parser.error("--exe must name the new RustView release executable")
    work = args.output.resolve().parent / "large-archive-psd-cases"
    work.mkdir(parents=True, exist_ok=True)
    source, archives, ordinary = prepare(work)
    originals = {path: digest(path) for path in [source, *archives.values(), ordinary]}
    if args.prepare_only:
        print(json.dumps({"prepared": True, "source_bytes": source.stat().st_size,
                          "archives": {name: str(path) for name, path in archives.items()}}))
        return 0
    temporary = work / "worker-temp"
    temporary.mkdir(exist_ok=True)
    if list(temporary.iterdir()):
        raise ValueError("Choose a fresh output folder: worker-temp is not empty")
    old_environment = {key: os.environ.get(key) for key in ["TEMP", "TMP"]}
    os.environ.update(TEMP=str(temporary), TMP=str(temporary))
    cases = []
    for name, archive in archives.items():
        source_request = load(archive)
        source_request.update(entry=ENTRY, password=PASSWORD if name.endswith("aes") else None)
        cases += [(name + "-list", {"operation": "List", "path": str(archive), "password": source_request["password"]}, "list")]
        cases += [(name + "-load", source_request, "image"),
                  (name + "-thumbnail", {"operation": "Thumbnail", "source": source_request}, "thumbnail"),
                  (name + "-page", {"operation": "Page", "source": source_request, "index": 999, "last": False}, "image")]
        destination = work / (name + "-composite.png")
        cases.append((name + "-render", render(source_request, save=destination, animation="AllFrames"), destination))
        if name.endswith("aes"):
            for label, password in [("missing", None), ("wrong", "incorrect-fixture-value")]:
                request = dict(source_request, password=password)
                cases.append((name + "-password-" + label, request, "password"))
    limited = dict(load(ordinary), entry="oversized.png")
    cases.append(("ordinary-image-keeps-input-limit", limited, "limit"))
    rar_password = PROJECT / "tests/fixtures/test_read_format_rar5_solid_encrypted_filenames.rar"
    for label, password in [("missing", None), ("wrong", "incorrect-fixture-value"), ("correct", "password")]:
        cases.append(("small-upstream-rar-password-" + label,
                      {"operation": "List", "path": str(rar_password), "password": password},
                      "list-small" if label == "correct" else "password"))
    cancellation_source = dict(load(archives["7z"]), entry=ENTRY)
    cases += [("cancel-large-7z-load", cancellation_source, "cancel"),
              ("cancel-large-7z-thumbnail", {"operation": "Thumbnail", "source": cancellation_source}, "cancel-thumbnail")]
    results = []
    try:
        for name, request, expected in cases:
            cancel = expected in ("cancel", "cancel-thumbnail")
            result = cancel_request(args.exe.resolve(), request, temporary) if cancel else run_request(args.exe.resolve(), request, 120)
            reply = result.pop("_full_reply", result.get("reply", {}))
            good = result["status"] == "pass" and not reply.get("error")
            if expected == "password":
                good = result["status"] == "fail" and bool(reply.get("error")) and bool(reply.get("password_required"))
            elif cancel:
                good = result["exit_code"] == 1 and reply.get("error") == "已取消"
                good &= 0 < result["observed_partial_bytes"] < source.stat().st_size
            elif expected == "limit":
                good = result["status"] == "fail" and "512" in reply.get("error", "") and not reply.get("password_required")
            elif expected == "list":
                entries = reply.get("entries") or []
                good &= any(item.get("name") == ENTRY and item.get("size", 0) > 512 * MIB for item in entries)
            elif isinstance(expected, Path):
                summary = reply.get("rendered") or {}
                good &= (summary.get("width"), summary.get("height"), summary.get("frames")) == (2, 1, 1)
                good &= expected.is_file() and orange_png(expected)
            elif expected in ("image", "thumbnail"):
                good &= (reply.get("width"), reply.get("height"), reply.get("page_total")) == (2, 1, 1)
                if expected == "thumbnail":
                    good &= (reply.get("source_width"), reply.get("source_height")) == (2, 1)
            peak = result["memory"]["peak_bytes"]
            ceiling = (576 if expected in ("thumbnail", "cancel-thumbnail") else 2176) * MIB
            good &= peak is None or peak <= ceiling
            for _ in range(20):
                if not list(temporary.iterdir()):
                    break
                time.sleep(0.05)
            cleaned = not list(temporary.iterdir())
            good &= cleaned
            result.update(case=name, status="pass" if good else "fail", worker_temp_clean=cleaned)
            results.append(result)
            print(json.dumps({"case": name, "status": result["status"], "peak_bytes": peak,
                              "seconds": result["elapsed_seconds"], "worker_temp_clean": cleaned}), flush=True)
    finally:
        for key, value in old_environment.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
    unchanged = all(digest(path) == before for path, before in originals.items())
    report = {"passed": sum(r["status"] == "pass" for r in results),
              "failed": sum(r["status"] != "pass" for r in results), "sources_unchanged": unchanged,
              "source_bytes": source.stat().st_size, "executable_sha256": digest(args.exe), "cases": results,
              "scope": "Synthetic large PSD in ZIP/RAR5-stored/7z and AES ZIP/7z; List/Load/Thumbnail/Page/PNG16 Render, budgets and temp cleanup. RAR password coverage uses the existing small upstream fixture. The probe cancellation hook exercises the real execute token during partial extraction, not UI interaction."}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"passed": report["passed"], "failed": report["failed"], "sources_unchanged": unchanged}), flush=True)
    return bool(report["failed"] or not unchanged)


if __name__ == "__main__":
    raise SystemExit(main())
