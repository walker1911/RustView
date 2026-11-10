"""Exercise real animation worker processes, archives, cancellation, and bounded memory."""

import argparse
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import struct
import subprocess
import threading
import time
import zipfile
import zlib

from smoke_test import stop_own_tree, scrub, TEST_PASSWORD, WRONG_PASSWORD, RAR_PASSWORD


PROJECT = Path(__file__).resolve().parent.parent
FLAGS = subprocess.CREATE_NO_WINDOW | subprocess.CREATE_NEW_PROCESS_GROUP if os.name == "nt" else 0


def private_tree_bytes(pid):
    """Sample committed private bytes for the probe and its current descendants."""
    if os.name != "nt":
        return None
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    psapi = ctypes.WinDLL("psapi", use_last_error=True)

    class Entry(ctypes.Structure):
        _fields_ = [("size", wintypes.DWORD), ("usage", wintypes.DWORD), ("pid", wintypes.DWORD),
                    ("heap", ctypes.c_size_t), ("module", wintypes.DWORD), ("threads", wintypes.DWORD),
                    ("parent", wintypes.DWORD), ("priority", wintypes.LONG), ("flags", wintypes.DWORD),
                    ("exe", wintypes.WCHAR * 260)]

    class Memory(ctypes.Structure):
        _fields_ = [("size", wintypes.DWORD), ("faults", wintypes.DWORD)] + [
            (name, ctypes.c_size_t) for name in ("peak_ws", "ws", "peak_paged", "paged", "peak_nonpaged",
                                               "nonpaged", "pagefile", "peak_pagefile", "private")]

    kernel.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
    kernel.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel.Process32FirstW.argtypes = kernel.Process32NextW.argtypes = [wintypes.HANDLE, ctypes.POINTER(Entry)]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel.OpenProcess.restype = wintypes.HANDLE
    psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(Memory), wintypes.DWORD]
    snapshot = kernel.CreateToolhelp32Snapshot(2, 0)
    if snapshot == ctypes.c_void_p(-1).value:
        return None
    parents = {}
    entry = Entry()
    entry.size = ctypes.sizeof(entry)
    try:
        success = kernel.Process32FirstW(snapshot, ctypes.byref(entry))
        while success:
            parents[entry.pid] = entry.parent
            success = kernel.Process32NextW(snapshot, ctypes.byref(entry))
    finally:
        kernel.CloseHandle(snapshot)
    descendants = {pid}
    while True:
        expanded = descendants | {child for child, parent in parents.items() if parent in descendants}
        if expanded == descendants:
            break
        descendants = expanded
    total = 0
    for process_id in descendants:
        handle = kernel.OpenProcess(0x410, False, process_id)
        if handle:
            try:
                counters = Memory()
                counters.size = ctypes.sizeof(counters)
                if psapi.GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.size):
                    total += counters.private
            finally:
                kernel.CloseHandle(handle)
    return total or None


def run_probe(exe, path, *, entry=None, password=None, frames=12, cancel_ms=None, sample=False, timeout=150):
    command = [str(exe), "--animation-probe", str(path), "--frames", str(frames)]
    if entry is not None:
        command += ["--entry", entry]
    if password is not None:
        command.append("--password-stdin")
    if cancel_ms is not None:
        command += ["--cancel-after-ms", str(cancel_ms)]
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               creationflags=FLAGS)
    result = {}
    done = threading.Event()

    def communicate():
        try:
            result["streams"] = process.communicate((password + "\n").encode() if password is not None else b"")
        except BaseException as error:
            result["exception"] = error
        finally:
            done.set()

    reader = threading.Thread(target=communicate, daemon=True)
    reader.start()
    started = time.monotonic()
    samples = []
    try:
        while not done.wait(0.05):
            if sample:
                memory = private_tree_bytes(process.pid)
                if memory is not None:
                    samples.append((round((time.monotonic() - started) * 1000), memory))
            if time.monotonic() - started > timeout:
                raise TimeoutError("Animation probe timed out")
    except BaseException:
        stop_own_tree(process)
        reader.join(timeout=10)
        raise
    if "exception" in result:
        raise result["exception"]
    stdout, stderr = result["streams"]
    reply = json.loads(stdout.decode("utf-8-sig"))
    return process.returncode, reply, samples, scrub(stderr.decode("utf-8", errors="replace")[:2048])


def validate(code, reply, expected, hashes=None):
    if not reply.get("finished") or reply.get("cleanup_ms", 10001) > 5000:
        return "Worker cancellation/cleanup did not finish within five seconds"
    if expected == "password":
        return None if code != 0 and reply.get("password_required") else "Expected a password classification"
    if expected == "not_image":
        return None if code != 0 and reply.get("error") and not reply.get("password_required") else "Encrypted RAR did not unlock before image rejection"
    if code != 0 or reply.get("error"):
        return "Animation worker failed"
    if expected == "cancel":
        return None if reply["reason"] == "cancelled" else "Cancellation deadline was not honored"
    if expected == "still":
        return None if reply["reason"] == "still" and reply["frame_count"] == 1 and not reply["frames"][0]["animated"] else "Static image incorrectly classified as animated"
    output = reply["frames"]
    if expected == "finite" and (reply["reason"] != "end" or len(output) != 6):
        return "Finite loop count did not produce exactly two three-frame plays"
    if expected == "infinite" and (reply["reason"] != "frame_limit" or len(output) != 1200):
        return "Infinite animation did not stream all requested frames"
    if [frame["duration_ms"] for frame in output] != [30, 50, 10] * (len(output) // 3):
        return "Frame delays changed or short-delay clamping failed"
    if any(not frame["animated"] or frame["width"] * frame["height"] * 4 > 128 * 1024 * 1024
           or max(frame["preview_width"], frame["preview_height"]) > 2048 for frame in output):
        return "An animation frame exceeded its dimension/output budget"
    actual = [frame["rgba_hash"] for frame in output]
    if len(actual) >= 6 and actual[:3] != actual[3:6]:
        return "Disposal canvas was not reset at the next loop"
    if hashes is not None and actual != hashes:
        return "Archive extraction changed decoded animation pixels"
    return None


def stored_rar(template, path, name, payload):
    """Derive a stored RAR4 test fixture; libarchive's original notice remains in fixtures."""
    data = template.read_bytes()
    start = 7 + int.from_bytes(data[12:14], "little")
    size = int.from_bytes(data[start + 5:start + 7], "little")
    header = bytearray(data[start:start + size])
    assert len(name.encode("ascii")) == int.from_bytes(header[26:28], "little")
    header[32:32 + len(name)] = name.encode("ascii")
    header[7:11] = header[11:15] = struct.pack("<I", len(payload))
    header[16:20] = struct.pack("<I", zlib.crc32(payload))
    header[:2] = struct.pack("<H", zlib.crc32(header[2:]) & 0xffff)
    path.write_bytes(data[:start] + header + payload + data[-7:])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, default=PROJECT / "test-output" / "animation-assets")
    parser.add_argument("--skip-memory", action="store_true")
    args = parser.parse_args()
    exe = args.exe.resolve()
    fixtures = PROJECT / "tests" / "fixtures"
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    sevenzip = exe.parent / "native" / "7zip" / "7z.exe"
    if not sevenzip.is_file():
        sevenzip = PROJECT / "native" / "7zip" / "7z.exe"
    cases = []

    def case(name, path, expected="finite", **options):
        try:
            code, reply, samples, stderr = run_probe(exe, path, **options)
            reason = validate(code, reply, expected)
            result = {"case": name, "status": "fail" if reason else "pass", "exit_code": code,
                      "reply": scrub({key: value for key, value in reply.items() if key != "frames"})}
            result["first_frames"] = reply.get("frames", [])[:6]
            if samples:
                # Warm-up and allocator growth are excluded by comparing steady-state quarters.
                quarter = max(1, len(samples) // 4)
                warm = max(value for _, value in samples[quarter:2 * quarter])
                tail = max(value for _, value in samples[-quarter:])
                growth = max(0, tail - warm)
                result["memory"] = {"metric": "probe + descendants private committed bytes", "sample_count": len(samples),
                                    "peak_bytes": max(value for _, value in samples), "warm_bytes": warm,
                                    "tail_bytes": tail, "growth_bytes": growth, "allowed_growth_bytes": 32 * 1024 * 1024}
                if len(samples) < 8:
                    reason = reason or "Insufficient memory samples"
                elif growth > 32 * 1024 * 1024:
                    reason = reason or "Private memory grew more than 32 MiB after warm-up"
            elif options.get("sample"):
                reason = reason or "Private memory counters were unavailable"
            if reason:
                result.update(status="fail", reason=reason, stderr=stderr)
            cases.append(result)
            return reply
        except Exception as error:
            cases.append({"case": name, "status": "fail", "reason": scrub(str(error))})
            return None

    def command(arguments, **options):
        completed = subprocess.run([str(sevenzip), *arguments], stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   creationflags=FLAGS, timeout=30, **options)
        if completed.returncode:
            raise RuntimeError("7-Zip fixture preparation failed: " + scrub(completed.stderr.decode(errors="replace")[:1024]))
        return completed.stdout

    references = {}
    for suffix, fixture in (("gif", "animation-previous.gif"), ("webp", "animation-blend.webp")):
        reply = case("direct_" + suffix, fixtures / fixture)
        references[suffix] = [frame["rgba_hash"] for frame in reply.get("frames", [])] if reply else None
    case("static_webp", fixtures / "test.webp", "still")
    archive = work / "unicode-animation.zip"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
        output.write(fixtures / "animation-previous.gif", "中文/动画.gif")
        output.write(fixtures / "animation-blend.webp", "中文/动画.webp")
    for suffix in ("gif", "webp"):
        reply = case("zip_" + suffix, archive, entry="中文/动画." + suffix)
        if reply and [frame["rgba_hash"] for frame in reply.get("frames", [])] != references[suffix]:
            cases[-1].update(status="fail", reason="ZIP pixels differ from original")
    for suffix, fixture, entry in (("gif", "animation-previous.gif", "test.gif"), ("webp", "animation-blend.webp", "tst.webp")):
        rar = work / ("stored-" + suffix + ".rar")
        payload = (fixtures / fixture).read_bytes()
        stored_rar(fixtures / "image.rar", rar, entry, payload)
        command(["t", "-bd", "-y", str(rar)])
        assert command(["x", "-so", "-bd", "-y", str(rar), entry]) == payload
        reply = case("rar_" + suffix, rar, entry=entry)
        if reply and [frame["rgba_hash"] for frame in reply.get("frames", [])] != references[suffix]:
            cases[-1].update(status="fail", reason="RAR pixels differ from original")
    for format_name, switches in (("7z", ["-t7z", "-mhe=on"]), ("zip", ["-tzip", "-mem=AES256"])):
        encrypted = work / ("encrypted-animation." + format_name)
        if encrypted.exists():
            encrypted.unlink()
        command(["a", "-bd", "-y", *switches, "-p" + TEST_PASSWORD, str(encrypted),
                 "animation-previous.gif", "animation-blend.webp"], cwd=fixtures)
        for suffix, name in (("gif", "animation-previous.gif"), ("webp", "animation-blend.webp")):
            case(format_name + "_missing_" + suffix, encrypted, "password", entry=name)
            case(format_name + "_wrong_" + suffix, encrypted, "password", entry=name, password=WRONG_PASSWORD)
            reply = case(format_name + "_correct_" + suffix, encrypted, entry=name, password=TEST_PASSWORD)
            if reply and [frame["rgba_hash"] for frame in reply.get("frames", [])] != references[suffix]:
                cases[-1].update(status="fail", reason="Password-protected archive changed pixels")
    rar = fixtures / "test_read_format_rar5_solid_encrypted_filenames.rar"
    case("rar5_encrypted_missing", rar, "password", entry="a.txt")
    case("rar5_encrypted_wrong", rar, "password", entry="a.txt", password=WRONG_PASSWORD)
    case("rar5_encrypted_unlocked_text_rejected", rar, "not_image", entry="a.txt", password=RAR_PASSWORD)
    case("cancel_infinite", fixtures / "animation-background.gif", "cancel", frames=10000, cancel_ms=75)
    if not args.skip_memory:
        large = bytearray((fixtures / "animation-background.gif").read_bytes())
        large[6:10] = struct.pack("<HH", 512, 512)
        large_path = work / "infinite-512.gif"
        large_path.write_bytes(large)
        case("bounded_1200_frames", large_path, "infinite", frames=1200, sample=True)
    report = {"executable": str(exe), "passed": sum(case["status"] == "pass" for case in cases),
              "failed": sum(case["status"] == "fail" for case in cases), "cases": cases,
              "scope": "Real GIF/WebP streaming, stored RAR4 and ZIP, encrypted ZIP/7z, encrypted RAR5 classification; existing encrypted RAR fixture contains text, not animation"}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(scrub(report), ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"passed": report["passed"], "failed": report["failed"], "report": str(args.output)}, ensure_ascii=False))
    return bool(report["failed"])


if __name__ == "__main__":
    raise SystemExit(main())
