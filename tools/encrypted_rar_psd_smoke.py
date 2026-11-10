"""Real large encrypted RAR PSD regressions; no private artwork or bundled RAR writer.

Pass an independently acquired official RAR CLI using --rar-exe. Generated source
documents, archives and traces stay under test-output; only this generator ships
in the source repository. See tests/fixtures/psd/ENCRYPTED-RAR.md.
"""
import argparse
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import threading
import time

from compat_smoke import FLAGS, MIB, digest, load, png_info, render, run_request
from large_archive_psd_smoke import write_large_psd


PROJECT = Path(__file__).resolve().parent.parent
PASSWORD = "fixture-password"
WIDTH, HEIGHT = 512, 384
ENTRIES = ["large-a.psd", "large-b.psd"]
SEED = "small-seed.png"
MARKERS = [(245, 90, 40), (40, 100, 245)]


def write_artwork(path, variant):
    write_large_psd(path)
    offset = path.stat().st_size - 8
    planes = [bytearray(WIDTH * HEIGHT) for _ in range(3)]
    for y in range(HEIGHT):
        for x in range(WIDTH):
            if x < 96 and y < 96:
                color = MARKERS[variant]
            elif variant == 0:
                color = (x * 255 // (WIDTH - 1), y * 255 // (HEIGHT - 1), 48)
            else:
                color = (48, x * 255 // (WIDTH - 1), y * 255 // (HEIGHT - 1))
            for channel in range(3):
                planes[channel][y * WIDTH + x] = color[channel]
    with path.open("r+b") as output:
        output.seek(14)
        output.write(struct.pack(">II", HEIGHT, WIDTH))
        output.seek(offset)
        output.write(b"\0\0")
        for plane in planes:
            output.write(plane)


def command(arguments, work):
    environment = os.environ.copy()
    for key in ["RAR", "RARINISWITCHES"]:
        environment.pop(key, None)
    result = subprocess.run(arguments, cwd=work, env=environment, stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            creationflags=FLAGS, timeout=180)
    if result.returncode:
        raise ValueError("Fixture command failed: " + (result.stdout + result.stderr).decode("utf-8", "replace")[:2500])
    return result.stdout.decode("utf-8", "replace")


def prepare(work, rar_exe):
    manifest_path = work / "fixtures.json"
    sources = [work / name for name in ENTRIES]
    archives = {"data": work / "large-encrypted-data.rar", "headers": work / "large-encrypted-headers.rar"}
    paths = [*sources, work / SEED, *archives.values()]
    if manifest_path.is_file():
        manifest = json.loads(manifest_path.read_text())
        if manifest.get("generator") == 2 and all(path.is_file() and digest(path) == manifest["sha256"].get(path.name) for path in paths):
            return sources, archives, manifest
        if manifest.get("generator") == 2:
            raise ValueError("Prepared fixtures differ; use a fresh output folder")
    for variant, source in enumerate(sources):
        write_artwork(source, variant)
    shutil.copy2(PROJECT / "tests/fixtures/image.png", work / SEED)
    for kind, archive in archives.items():
        if archive.exists():
            archive.unlink()
        switch = "-p" if kind == "data" else "-hp"
        command([str(rar_exe), "a", "-cfg-", "-m1", "-md1m", "-mt1", "-s-", "-ep", "-idq", "-y",
                 switch + PASSWORD, str(archive), *ENTRIES, SEED], work)
        with archive.open("rb") as archive_file:
            signature = archive_file.read(8)
        if signature != b"Rar!\x1a\x07\x01\0":
            raise ValueError("Fixture is not RAR5")
        command([str(PROJECT / "native/7zip/7z.exe"), "t", "-p" + PASSWORD, str(archive)], work)
    manifest = {"generator": 2, "rar_exe_sha256": digest(rar_exe),
                "dimensions": [WIDTH, HEIGHT], "source_sizes": [p.stat().st_size for p in sources],
                "sha256": {p.name: digest(p) for p in paths},
                "compression": "RAR5 m1, 1 MiB dictionary, single thread, non-solid",
                "encryption": {"data": "RAR -p: file data", "headers": "RAR -hp: file data and names"}}
    manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    return sources, archives, manifest


def prepare_stored(work, rar_exe):
    manifest_path = work / "stored-fixtures.json"
    archives = {"stored-data": work / "large-stored-data.rar", "stored-headers": work / "large-stored-headers.rar"}
    if manifest_path.is_file():
        manifest = json.loads(manifest_path.read_text())
        if all(path.is_file() and digest(path) == manifest["sha256"].get(path.name) for path in archives.values()):
            return archives, manifest
        raise ValueError("Stored fixtures differ; choose a fresh output folder")
    for kind, archive in archives.items():
        switch = "-hp" if kind.endswith("headers") else "-p"
        command([str(rar_exe), "a", "-cfg-", "-m0", "-md1m", "-mt1", "-s-", "-ep", "-idq", "-y",
                 switch + PASSWORD, str(archive), ENTRIES[0], SEED], work)
        if archive.stat().st_size <= 512 * MIB:
            raise ValueError("Stored encrypted RAR must physically exceed 512 MiB")
        command([str(PROJECT / "native/7zip/7z.exe"), "t", "-p" + PASSWORD, str(archive)], work)
    manifest = {"rar_exe_sha256": digest(rar_exe), "compression": "RAR5 m0 stored, single thread, non-solid",
                "sha256": {path.name: digest(path) for path in archives.values()},
                "physical_archive_bytes": {kind: path.stat().st_size for kind, path in archives.items()}}
    manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    return archives, manifest


class ProcessEntry(ctypes.Structure):
    _fields_ = [("size", wintypes.DWORD), ("usage", wintypes.DWORD), ("pid", wintypes.DWORD),
                ("heap", ctypes.c_size_t), ("module", wintypes.DWORD), ("threads", wintypes.DWORD),
                ("parent", wintypes.DWORD), ("priority", wintypes.LONG), ("flags", wintypes.DWORD),
                ("exe", wintypes.WCHAR * 260)]


def within_parent_lifetime(child_created, parent_created, parent_exited):
    return child_created >= parent_created and (not parent_exited or child_created <= parent_exited)


def tracker_self_test():
    assert not within_parent_lifetime(99, 100, 0)  # Historical child of a reused PID.
    assert not within_parent_lifetime(201, 100, 200)  # New child after this parent exited.
    assert within_parent_lifetime(100, 100, 0)
    assert within_parent_lifetime(150, 100, 200)
    assert within_parent_lifetime(200, 100, 200)


class ProcessTracker:
    """Hold observed descendants' process handles and verify their signaled state."""
    def __init__(self):
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.kernel.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
        self.kernel.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
        self.kernel.Process32FirstW.argtypes = self.kernel.Process32NextW.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)]
        self.kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        self.kernel.OpenProcess.restype = wintypes.HANDLE
        self.kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
        self.kernel.WaitForSingleObject.restype = wintypes.DWORD
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        self.kernel.GetProcessTimes.argtypes = [wintypes.HANDLE, *([ctypes.POINTER(wintypes.FILETIME)] * 4)]
        self.psapi = ctypes.WinDLL("psapi", use_last_error=True)
        self.psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD]
        self.origin = self.open_process(os.getpid())
        if self.origin is None:
            raise ctypes.WinError(ctypes.get_last_error())
        self.seen, self.root = {}, None

    def times(self, handle):
        values = [wintypes.FILETIME() for _ in range(4)]
        if not self.kernel.GetProcessTimes(handle, *[ctypes.byref(value) for value in values]):
            raise ctypes.WinError(ctypes.get_last_error())
        return tuple((value.dwHighDateTime << 32) | value.dwLowDateTime for value in values[:2])

    def open_process(self, pid):
        # Hold the exact process instance for ancestry, memory and exit checks.
        handle = self.kernel.OpenProcess(0x101410, False, pid)  # SYNCHRONIZE + QUERY + VM_READ
        if not handle:
            return None  # A short-lived snapshot entry can disappear before opening.
        try:
            created, _ = self.times(handle)
        except BaseException:
            self.kernel.CloseHandle(handle)
            raise
        return {"handle": handle, "created": created}

    def table(self):
        snapshot = self.kernel.CreateToolhelp32Snapshot(2, 0)
        if snapshot == ctypes.c_void_p(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())
        rows = {}
        entry = ProcessEntry()
        entry.size = ctypes.sizeof(entry)
        try:
            present = self.kernel.Process32FirstW(snapshot, ctypes.byref(entry))
            while present:
                rows[entry.pid] = (entry.parent, entry.exe)
                present = self.kernel.Process32NextW(snapshot, ctypes.byref(entry))
        finally:
            self.kernel.CloseHandle(snapshot)
        return rows

    def observe(self):
        rows = self.table()
        known = {os.getpid(): self.origin, **self.seen}
        while True:
            added = False
            for pid, (parent, name) in rows.items():
                if pid in known or parent not in known:
                    continue
                data = self.open_process(pid)
                if data is None:
                    continue
                parent_created, parent_exited = self.times(known[parent]["handle"])
                if not within_parent_lifetime(data["created"], parent_created, parent_exited):
                    self.kernel.CloseHandle(data["handle"])
                    continue
                data.update(name=name, parent=parent)
                known[pid] = self.seen[pid] = data
                added = True
                if parent == os.getpid():
                    if self.root is not None and self.root != pid:
                        raise ValueError("More than one direct probe process was observed")
                    self.root = pid
            if not added:
                break

    def private_bytes(self):
        class Memory(ctypes.Structure):
            _fields_ = [("size", wintypes.DWORD), ("faults", wintypes.DWORD)] + [
                (name, ctypes.c_size_t) for name in ("peak_ws", "ws", "peak_paged", "paged", "peak_nonpaged",
                                                   "nonpaged", "pagefile", "peak_pagefile", "private")]
        total = 0
        for data in self.seen.values():
            if self.kernel.WaitForSingleObject(data["handle"], 0) == 0:
                continue
            counters = Memory()
            counters.size = ctypes.sizeof(counters)
            if self.psapi.GetProcessMemoryInfo(data["handle"], ctypes.byref(counters), counters.size):
                total += counters.private
        return total or None

    def finish(self):
        started = time.monotonic()
        while True:
            self.observe()
            alive = [pid for pid, data in self.seen.items()
                     if self.kernel.WaitForSingleObject(data["handle"], 0) != 0]
            if not alive or time.monotonic() - started > 2:
                break
            time.sleep(0.02)
        records = [{"pid": pid, "name": data["name"], "parent_pid": data["parent"],
                    "created_filetime": data["created"], "exited": pid not in alive,
                    "verification": "creation-time ancestry and process handle signaled"}
                   for pid, data in self.seen.items()]
        for data in self.seen.values():
            if data["handle"]:
                self.kernel.CloseHandle(data["handle"])
        self.kernel.CloseHandle(self.origin["handle"])
        return records, alive, round((time.monotonic() - started) * 1000)


def cancel_probe(exe, request, delay_ms):
    started = time.monotonic()
    result = subprocess.run([str(exe), "--request-probe", "--cancel-after-ms", str(delay_ms)],
                            input=json.dumps(request).encode("utf-8"), stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, creationflags=FLAGS, timeout=20)
    return {"status": "fail", "exit_code": result.returncode,
            "reply": json.loads(result.stdout.decode("utf-8-sig")),
            "elapsed_seconds": round(time.monotonic() - started, 3), "cancel_after_ms": delay_ms}


def tracked_request(exe, request, temporary, cancel, cancel_ms=300):
    tracker = ProcessTracker()
    done, saved, samples, partial = threading.Event(), {}, [], 0

    def execute():
        try:
            saved["result"] = cancel_probe(exe, request, cancel_ms) if cancel else run_request(exe, request, 120)
        except BaseException as error:
            saved["error"] = error
        finally:
            done.set()

    runner = threading.Thread(target=execute, daemon=True)
    runner.start()
    while True:
        tracker.observe()
        if tracker.root is not None:
            memory = tracker.private_bytes()
            if memory is not None:
                samples.append(memory)
        try:
            inputs = list(temporary.rglob("input.psd"))
        except FileNotFoundError:
            inputs = []  # The worker can remove its directory during observation.
        for path in inputs:
            try:
                partial = max(partial, path.stat().st_size)
            except FileNotFoundError:
                pass
        if done.wait(0.02):
            break
    runner.join()
    processes, alive, wait_ms = tracker.finish()
    if "error" in saved:
        raise saved["error"]
    result = saved["result"]
    result.update(processes=processes, surviving_pids=alive, processes_wait_ms=wait_ms,
                  observed_partial_bytes=partial,
                  memory={"metric": "probe + descendants private committed bytes; excludes Python and OS file cache",
                          "interval_ms": 20, "sample_count": len(samples),
                          "peak_bytes": max(samples) if samples else None})
    return result


def marker_matches(path, variant):
    if path.stat().st_size > 8 * MIB:
        return False
    info = png_info(path)
    if (info["width"], info["height"], info["bit_depth"]) != (WIDTH, HEIGHT, 16):
        return False
    if info["color_type"] not in (2, 6) or info["interlace"]:
        return False
    import zlib
    data, compressed, offset = path.read_bytes(), bytearray(), 8
    while offset < len(data):
        length, kind = struct.unpack_from(">I4s", data, offset)
        if kind == b"IDAT":
            compressed.extend(data[offset + 8:offset + 8 + length])
        offset += length + 12
    channels = 3 if info["color_type"] == 2 else 4
    raw = zlib.decompressobj().decompress(compressed, 1 + channels * 2)
    if len(raw) != 1 + channels * 2 or raw[0] > 4:
        return False
    # First pixel of the first non-interlaced row has zero filter neighbours.
    values = struct.unpack(">" + "H" * channels, raw[1:])
    expected = MARKERS[variant] + ((255,) if channels == 4 else ())
    return all(abs(value / 257 - reference) < 1 for value, reference in zip(values, expected))


def run_cases(exe, output_path, work, sources, archives, manifest):
    cases = []
    for kind, archive in archives.items():
        stored = kind.startswith("stored-")
        for label, password in [("missing", None), ("wrong", "incorrect-fixture-value"), ("correct", PASSWORD)]:
            expected = "password" if kind.endswith("headers") and password != PASSWORD else "list"
            cases.append((kind + "-list-" + label, {"operation": "List", "path": str(archive), "password": password}, expected, 0))
        for label, password in [("missing", None), ("wrong", "incorrect-fixture-value")]:
            request = dict(load(archive), entry=ENTRIES[0], password=password)
            cases += [(kind + "-load-" + label, request, "password", 0),
                      (kind + "-thumbnail-" + label, {"operation": "Thumbnail", "source": request}, "password", 0)]
        for variant, entry in enumerate(ENTRIES):
            if stored and variant != 0:
                continue
            request = dict(load(archive), entry=entry, password=PASSWORD)
            cases += [(f"{kind}-load-{variant}", request, "image", variant),
                      (f"{kind}-thumbnail-{variant}", {"operation": "Thumbnail", "source": request}, "thumbnail", variant),
                      (f"{kind}-page-{variant}", {"operation": "Page", "source": request, "index": 999, "last": False}, "image", variant),
                      (f"{kind}-render-{variant}", render(request, save=work / f"{kind}-{variant}.png", animation="AllFrames"), "render", variant)]
        for step in range(0 if stored else 8):
            variant = (step // 2) % 2
            request = dict(load(archive), entry=ENTRIES[variant], password=PASSWORD)
            thumbnail = step >= 4
            if thumbnail:
                request = {"operation": "Thumbnail", "source": request}
            expected = "cancel" if step % 2 == 0 else ("thumbnail" if thumbnail else "image")
            cases.append((f"{kind}-churn-{step}", request, expected, variant))
        for thumbnail in [False, True]:
            variant = 0 if stored else int(thumbnail)
            request = dict(load(archive), entry=ENTRIES[variant], password=PASSWORD)
            if thumbnail:
                request = {"operation": "Thumbnail", "source": request}
            cases.append((kind + "-early-cancel-" + ("thumbnail" if thumbnail else "load"), request, "cancel-early", variant))
    results = []
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps({"status": "running", "planned_cases": len(cases)}), encoding="utf-8")
    old_environment = {key: os.environ.get(key) for key in ["TEMP", "TMP"]}
    try:
        for number, (name, request, expected, variant) in enumerate(cases):
            temporary = work / "request-temp" / f"{number:02}-{name}"
            temporary.mkdir(parents=True, exist_ok=True)
            if list(temporary.iterdir()):
                raise ValueError("Request temporary folder is not empty; choose a fresh output folder")
            os.environ.update(TEMP=str(temporary), TMP=str(temporary))
            cancelling = expected in ("cancel", "cancel-early")
            result = tracked_request(exe, request, temporary, cancelling, cancel_ms=150 if expected == "cancel-early" else 300)
            reply = result.pop("_full_reply", result.get("reply", {}))
            good = result["status"] == "pass" and not reply.get("error")
            if expected == "password":
                good = result["exit_code"] != 0 and bool(reply.get("error")) and bool(reply.get("password_required"))
            elif cancelling:
                good = result["exit_code"] == 1 and reply.get("error") == "已取消" and result["elapsed_seconds"] < 3
                if expected == "cancel-early":
                    good &= 0 < result["observed_partial_bytes"] < sources[variant].stat().st_size
            elif expected == "list":
                entries = reply.get("entries") or []
                wanted = ENTRIES[:1] if Path(request["path"]).name.startswith("large-stored-") else ENTRIES
                good &= all(any(item.get("name") == entry and item.get("size", 0) > 512 * MIB for item in entries) for entry in wanted)
                good &= any(item.get("name") == SEED for item in entries)
            elif expected == "render":
                summary = reply.get("rendered") or {}
                good &= (summary.get("width"), summary.get("height"), summary.get("frames")) == (WIDTH, HEIGHT, 1)
                destination = Path(request["save"])
                good &= destination.is_file() and marker_matches(destination, variant)
            elif expected == "thumbnail":
                good &= 0 < reply.get("width", 0) <= 128 and 0 < reply.get("height", 0) <= 128
                good &= (reply.get("source_width"), reply.get("source_height"), reply.get("page_total")) == (WIDTH, HEIGHT, 1)
            elif expected == "image":
                good &= (reply.get("width"), reply.get("height"), reply.get("page_total")) == (WIDTH, HEIGHT, 1)
            peak = result["memory"]["peak_bytes"]
            ceiling = (576 if request["operation"] == "Thumbnail" else 2176) * MIB
            names = {item["name"].lower() for item in result["processes"]}
            good &= bool(result["processes"]) and not result["surviving_pids"]
            if expected in ("image", "thumbnail", "render", "cancel", "cancel-early"):
                good &= "7z.exe" in names
            good &= peak is not None and peak <= ceiling
            for _ in range(20):
                if not list(temporary.iterdir()):
                    break
                time.sleep(0.05)
            cleaned = not list(temporary.iterdir())
            good &= cleaned
            spool_bytes = result["observed_partial_bytes"]
            stage = "not_observed" if spool_bytes == 0 else (
                "partial_selected_entry" if spool_bytes < sources[variant].stat().st_size else "complete_selected_entry")
            result.update(case=name, expected=expected, status="pass" if good else "fail", temporary_clean=cleaned,
                          extraction_observation=stage)
            results.append(result)
            print(json.dumps({"case": name, "status": result["status"], "seconds": result["elapsed_seconds"],
                              "peak_bytes": peak, "observed_processes": len(result["processes"]),
                              "surviving_pids": result["surviving_pids"], "temporary_clean": cleaned}), flush=True)
    finally:
        for key, value in old_environment.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
    unchanged = all(digest(work / name) == value for name, value in manifest["sha256"].items())
    report = {"passed": sum(item["status"] == "pass" for item in results),
              "failed": sum(item["status"] != "pass" for item in results),
              "fixtures_unchanged": unchanged, "fixtures": manifest, "executable_sha256": digest(exe),
              "cases": results,
              "scope": "RAR5 data/header encryption on >512 MiB synthetic PSDs: m1 compressed-small containers and m0 physically-large encrypted containers are reported separately. Per-request TEMP isolation, observed descendant handles waited to exit, memory sampling and repeated cancel/reload. Independent worker probes are not same-Viewer GUI navigation. Sampling may miss very short-lived descendants; all observed PIDs are verified, and long-running 7z must be observed for decoded/cancelled cases."}
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"passed": report["passed"], "failed": report["failed"], "fixtures_unchanged": unchanged}), flush=True)
    return bool(report["failed"] or not unchanged)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rar-exe", type=Path)
    parser.add_argument("--exe", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--prepare-only", action="store_true")
    parser.add_argument("--tracker-self-test", action="store_true")
    args = parser.parse_args()
    if args.tracker_self_test:
        tracker_self_test()
        print("PASS: process ancestry rejects children outside the held parent's lifetime")
        return 0
    if os.name != "nt":
        parser.error("The process-handle checks require Windows")
    if args.rar_exe is None or not args.rar_exe.is_file():
        parser.error("--rar-exe must name an independently acquired official RAR CLI")
    if args.output is None:
        parser.error("--output is required")
    if not args.prepare_only and (args.exe is None or not args.exe.is_file()):
        parser.error("--exe must name RustView.exe")
    work = args.output.resolve().parent / "encrypted-rar-psd-cases"
    work.mkdir(parents=True, exist_ok=True)
    sources, archives, manifest = prepare(work, args.rar_exe.resolve())
    stored_archives, stored_manifest = prepare_stored(work, args.rar_exe.resolve())
    archives.update(stored_archives)
    manifest = {**manifest, "stored": stored_manifest,
                "sha256": {**manifest["sha256"], **stored_manifest["sha256"]},
                "physical_archive_bytes": {kind: path.stat().st_size for kind, path in archives.items()}}
    if args.prepare_only:
        print(json.dumps({"prepared": True, "archives": {k: str(v) for k, v in archives.items()},
                          "source_sizes": manifest["source_sizes"], "dimensions": manifest["dimensions"]}), flush=True)
        return 0
    return run_cases(args.exe.resolve(), args.output, work, sources, archives, manifest)


if __name__ == "__main__":
    raise SystemExit(main())
