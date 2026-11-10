"""Exercise RustView's real isolated-worker pipeline using only the Python standard library."""

import argparse
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import time
import zipfile
import zlib


PROJECT = Path(__file__).resolve().parent.parent
TEST_PASSWORD = "fixture-password"
RAR_PASSWORD = "password"
WRONG_PASSWORD = "incorrect-test-value"


def png_chunk(output, kind, data):
    output.write(struct.pack(">I", len(data)))
    output.write(kind)
    output.write(data)
    output.write(struct.pack(">I", zlib.crc32(kind + data)))


def write_png(path, width, height, transparent=False):
    """Stream rows through zlib with bounded row buffers, never a full-image pixel buffer."""
    with path.open("wb") as output:
        output.write(b"\x89PNG\r\n\x1a\n")
        png_chunk(output, b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        png_chunk(output, b"sRGB", b"\x00")
        compressor = zlib.compressobj(level=1)
        for y in range(height):
            upper = y < height // 2
            left = bytes((240, 45, 30, 255 if not transparent else 0)) if upper else bytes((30, 75, 230, 255))
            right = bytes((40, 210, 70, 255 if not transparent else 96)) if upper else bytes((240, 200, 20, 255))
            row = b"\x00" + left * (width // 2) + right * (width - width // 2)
            compressed = compressor.compress(row)
            if compressed:
                png_chunk(output, b"IDAT", compressed)
        compressed = compressor.flush()
        if compressed:
            png_chunk(output, b"IDAT", compressed)
        png_chunk(output, b"IEND", b"")


def stop_own_tree(process):
    if process.poll() is not None:
        return
    if os.name == "nt":
        # This PID belongs to the Popen instance created immediately above, never a process-name search.
        subprocess.run(
            ["taskkill.exe", "/PID", str(process.pid), "/T", "/F"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            creationflags=subprocess.CREATE_NO_WINDOW,
            timeout=10,
            check=False,
        )
    else:
        process.kill()
    if process.poll() is None:
        process.kill()


def scrub(value):
    if isinstance(value, str):
        for secret in (TEST_PASSWORD, RAR_PASSWORD, WRONG_PASSWORD):
            value = value.replace(secret, "<redacted>")
        return value
    if isinstance(value, list):
        return [scrub(item) for item in value]
    if isinstance(value, dict):
        return {key: scrub(item) for key, item in value.items()}
    return value


def check_reply(reply, returncode, expected):
    kind = expected["kind"]
    if kind == "error":
        if returncode == 0 or not reply.get("error"):
            return "The invalid or encrypted input unexpectedly succeeded"
        password_required = expected.get("password_required")
        if password_required is not None and bool(reply.get("password_required")) != password_required:
            return "The password/error classification did not match"
        fragment = expected.get("message_contains")
        if fragment and fragment not in str(reply["error"]):
            return "The expected rejection reason was not returned"
        return None
    if returncode != 0 or reply.get("error"):
        return "The supported input failed"
    if kind == "list":
        entries = reply.get("entries")
        if not isinstance(entries, list):
            return "No archive entry list was returned"
        names = {entry.get("name") for entry in entries}
        if not set(expected.get("names", ())).issubset(names):
            return "The archive did not expose the expected image names"
        return None
    if (reply.get("width"), reply.get("height")) != tuple(expected["size"]):
        return "The decoded dimensions did not match"
    preview_width = reply.get("preview_width", 0)
    preview_height = reply.get("preview_height", 0)
    if not (0 < preview_width <= 2048 and 0 < preview_height <= 2048):
        return "The bounded preview was missing or oversized"
    return None


def run_case(executable, timeout, name, path, expected, entry=None, password=None, full_raw=False):
    arguments = [str(executable), "--probe", str(path)]
    if entry is not None:
        arguments.extend(("--entry", entry))
    if password is not None:
        arguments.append("--password-stdin")
    if full_raw:
        arguments.append("--full-raw")
    flags = subprocess.CREATE_NO_WINDOW | subprocess.CREATE_NEW_PROCESS_GROUP if os.name == "nt" else 0
    start = time.monotonic()
    process = subprocess.Popen(
        arguments,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        creationflags=flags,
    )
    try:
        output, errors = process.communicate(
            ((password + "\n").encode("utf-8") if password is not None else b""), timeout=timeout
        )
    except subprocess.TimeoutExpired:
        try:
            stop_own_tree(process)
        finally:
            try:
                process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)
        return {"case": name, "status": "fail", "reason": "Timed out; this test's process tree was stopped", "duration_ms": round((time.monotonic() - start) * 1000)}
    except BaseException:
        stop_own_tree(process)
        process.wait(timeout=10)
        raise
    duration = round((time.monotonic() - start) * 1000)
    text = output.decode("utf-8-sig", errors="replace").strip()
    try:
        reply = json.loads(text)
    except (ValueError, TypeError):
        return {"case": name, "status": "fail", "reason": "The probe did not return UTF-8 JSON", "exit_code": process.returncode, "duration_ms": duration, "stderr": scrub(errors.decode("utf-8", errors="replace")[:4096]), "stdout": scrub(text[:4096])}
    reason = check_reply(reply, process.returncode, expected)
    result = {"case": name, "status": "fail" if reason else "pass", "exit_code": process.returncode, "duration_ms": duration, "reply": scrub(reply)}
    if reason:
        result["reason"] = reason
        result["stderr"] = scrub(errors.decode("utf-8", errors="replace")[:4096])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", required=True, type=Path, help="The RustView.exe build to exercise")
    parser.add_argument("--output", required=True, type=Path, help="JSON report destination")
    parser.add_argument("--fixtures", type=Path, default=PROJECT / "tests" / "fixtures")
    parser.add_argument("--work-dir", type=Path, default=PROJECT / "test-output" / "smoke-assets")
    parser.add_argument("--timeout", type=float, default=150, help="Per-case wall time in seconds")
    parser.add_argument("--skip-large", action="store_true", help="Skip the 12000×8000 PNG case for a quick check")
    args = parser.parse_args()
    executable = args.exe.resolve()
    if not executable.is_file():
        parser.error("--exe must name an existing build")
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    args.work_dir.mkdir(parents=True, exist_ok=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    fixtures = args.fixtures.resolve()
    blocks = args.work_dir / "rgba-blocks.png"
    transparent = args.work_dir / "transparent.png"
    write_png(blocks, 64, 48)
    write_png(transparent, 64, 48, transparent=True)
    split_rar = args.work_dir / "unsupported.part01.rar"
    shutil.copyfile(fixtures / "image.rar", split_rar)
    invalid = args.work_dir / "invalid.png"
    invalid.write_bytes(b"not an image")
    unicode_zip = args.work_dir / "unicode-entry.zip"
    with zipfile.ZipFile(unicode_zip, "w") as archive:
        archive.writestr("@中文图.png", (fixtures / "image.png").read_bytes())
    image = lambda size: {"kind": "image", "size": size}
    listing = lambda names=(): {"kind": "list", "names": names}
    needs_password = {"kind": "error", "password_required": True}
    cases = [
        ("png_rgba_blocks", blocks, image((64, 48)), {}),
        ("png_transparency", transparent, image((64, 48)), {}),
        ("webp_lossless_rgba", fixtures / "test.webp", image((2, 2)), {}),
        ("plain_zip_index", fixtures / "plain.zip", listing(("image.png",)), {}),
        ("plain_zip_image", fixtures / "plain.zip", image((1, 1)), {"entry": "image.png"}),
        ("zip_unicode_literal_entry", unicode_zip, image((1, 1)), {"entry": "@中文图.png"}),
        ("plain_7z_index", fixtures / "plain.7z", listing(("image.png",)), {}),
        ("plain_7z_image", fixtures / "plain.7z", image((1, 1)), {"entry": "image.png"}),
        ("plain_rar_image", fixtures / "image.rar", image((1, 1)), {"entry": "test.tga"}),
        ("aes_zip_missing_password", fixtures / "aes.zip", needs_password, {"entry": "image.png"}),
        ("aes_zip_wrong_password", fixtures / "aes.zip", needs_password, {"entry": "image.png", "password": WRONG_PASSWORD}),
        ("aes_zip_correct_password", fixtures / "aes.zip", image((1, 1)), {"entry": "image.png", "password": TEST_PASSWORD}),
        ("zipcrypto_correct_password", fixtures / "encrypted.zip", image((1, 1)), {"entry": "image.png", "password": TEST_PASSWORD}),
        ("encrypted_7z_missing_password", fixtures / "encrypted.7z", needs_password, {}),
        ("encrypted_7z_wrong_password", fixtures / "encrypted.7z", needs_password, {"password": WRONG_PASSWORD}),
        ("encrypted_7z_correct_index", fixtures / "encrypted.7z", listing(("image.png",)), {"password": TEST_PASSWORD}),
        ("encrypted_7z_correct_image", fixtures / "encrypted.7z", image((1, 1)), {"entry": "image.png", "password": TEST_PASSWORD}),
        ("encrypted_rar5_missing_password", fixtures / "test_read_format_rar5_solid_encrypted_filenames.rar", needs_password, {}),
        ("encrypted_rar5_wrong_password", fixtures / "test_read_format_rar5_solid_encrypted_filenames.rar", needs_password, {"password": WRONG_PASSWORD}),
        # The upstream RAR encryption fixtures contain text only, so the image-only index is empty.
        ("encrypted_rar5_correct_header_password", fixtures / "test_read_format_rar5_solid_encrypted_filenames.rar", listing(), {"password": RAR_PASSWORD}),
        ("raw_full_decode", PROJECT / "native" / "LibRaw" / "testdata" / "synthetic.dng", image((32, 24)), {"full_raw": True}),
        ("split_rar_rejected", split_rar, {"kind": "error", "password_required": False, "message_contains": "分卷"}, {}),
        ("invalid_image_rejected", invalid, {"kind": "error", "password_required": False}, {}),
        ("valid_image_after_failures", blocks, image((64, 48)), {}),
    ]
    if not args.skip_large:
        large = args.work_dir / "large-12000x8000.png"
        write_png(large, 12000, 8000)
        cases.insert(3, ("large_96mp_png", large, image((12000, 8000)), {}))
    results = []
    for name, path, expected, options in cases:
        if not path.is_file():
            result = {"case": name, "status": "fail", "reason": "Required fixture is missing", "fixture": str(path)}
        else:
            try:
                result = run_case(executable, args.timeout, name, path, expected, **options)
            except Exception as error:
                result = {"case": name, "status": "fail", "reason": scrub(str(error))}
        results.append(result)
        print(f"{result['status'].upper()}: {name}", flush=True)
    passed = sum(result["status"] == "pass" for result in results)
    report = {"executable": str(executable), "passed": passed, "failed": len(results) - passed, "total": len(results), "large_fixture_enabled": not args.skip_large, "results": results}
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"{passed}/{len(results)} passed; JSON report written", flush=True)
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
