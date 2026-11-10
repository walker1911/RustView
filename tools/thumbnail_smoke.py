"""Check the real 512 MiB thumbnail-worker path, independently of the GUI."""
import argparse
import json
from pathlib import Path

from compat_smoke import load, run_request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--samples", type=Path)
    args = parser.parse_args()
    project = Path(__file__).resolve().parent.parent
    fixtures = project / "tests" / "fixtures"
    cases = []
    for name in ["image.png", "test.webp", "animation-previous.gif", "animation-blend.webp"]:
        cases.append((name, load(fixtures / name), False))
    for name in ["plain.zip", "plain.7z", "image.rar"]:
        source = load(fixtures / name)
        source["entry"] = "test.tga" if name.endswith(".rar") else "image.png"
        cases.append((name, source, False))
    for password, needs_password in [(None, True), ("fixture-password", False)]:
        source = load(fixtures / "encrypted.7z")
        source.update(entry="image.png", password=password)
        cases.append(("encrypted-7z-" + ("missing" if needs_password else "correct"), source, needs_password))
    if args.samples:
        for name in ["Nikon-Z6.NEF", "Sony-a7III.ARW", "Canon-EOS-R.CR3", "Apple-iPhone-12-Pro.DNG", "libheif-example.heic"]:
            cases.append((name, load(args.samples / name), False))
    results = []
    for name, source, needs_password in cases:
        result = run_request(args.exe.resolve(), {"operation": "Thumbnail", "source": source}, 40)
        reply = result.pop("_full_reply", result.get("reply", {}))
        valid = bool(reply.get("password_required")) if needs_password else (
            result["status"] == "pass" and not reply.get("error")
            and 0 < reply.get("width", 0) <= 128 and 0 < reply.get("height", 0) <= 128
            and reply.get("source_width", 0) >= reply.get("width", 1)
            and reply.get("source_height", 0) >= reply.get("height", 1)
        )
        peak = result["memory"]["peak_bytes"]
        if peak is not None and peak > 576 * 1024 * 1024:
            valid = False
            result["reason"] = "Thumbnail tree plus probe exceeded the sampled allowance"
        result.update(case=name, status="pass" if valid else "fail")
        results.append(result)
        print(json.dumps({"case": name, "status": result["status"], "peak_bytes": peak}), flush=True)
    report = {"passed": sum(r["status"] == "pass" for r in results),
              "failed": sum(r["status"] != "pass" for r in results), "cases": results,
              "scope": "Actual Thumbnail request supervisor and worker; process-tree committed memory excludes GUI"}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return bool(report["failed"])


if __name__ == "__main__":
    raise SystemExit(main())
