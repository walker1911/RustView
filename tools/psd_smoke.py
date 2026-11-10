"""Check PSD/PSB composites through the real bounded RustView worker."""
import argparse
import json
from pathlib import Path
import subprocess
import zipfile

from compat_smoke import FLAGS, digest, load, render, run_request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    project = Path(__file__).resolve().parent.parent
    fixtures = project / "tests" / "fixtures" / "psd"
    work = args.output.resolve().parent / "psd-cases"
    work.mkdir(parents=True, exist_ok=True)
    originals = {path: digest(path) for path in fixtures.glob("*.ps*")}
    cases = [(path.name, load(path), "image") for path in originals]
    for name in ["rgba8.psd", "layered.psd", "layered.psb"]:
        cases.append(("thumbnail-" + name, {"operation": "Thumbnail", "source": load(fixtures / name)}, "thumbnail"))
    archive = work / "photoshop.zip"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
        for name in ["layered.psd", "layered.psb"]:
            output.write(fixtures / name, name)
    cases.append(("zip-list", {"operation": "List", "path": str(archive), "password": None}, "list"))
    source = load(archive)
    source["entry"] = "layered.psd"
    cases.append(("zip-psd", source, "image"))
    source = load(archive)
    source["entry"] = "layered.psb"
    cases.append(("zip-psb-thumbnail", {"operation": "Thumbnail", "source": source}, "thumbnail"))
    encrypted = work / "photoshop-encrypted.7z"
    subprocess.run([str(project / "native" / "7zip" / "7z.exe"), "a", "-t7z", "-pfixture-password",
                    "-mhe=on", str(encrypted), "layered.psd"], cwd=fixtures,
                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, check=True, creationflags=FLAGS)
    source = load(encrypted)
    source["entry"] = "layered.psd"
    cases.append(("7z-password-required", dict(source), "password"))
    source["password"] = "fixture-password"
    cases.append(("7z-psd", dict(source), "image"))
    cases.append(("7z-psd-thumbnail", {"operation": "Thumbnail", "source": source}, "thumbnail"))
    for name in ["layered.psd", "layered.psb"]:
        for format_name, extension in [("Png", "png"), ("WebP", "webp")]:
            request = render(load(fixtures / name), save=work / f"{name}.{extension}",
                             format_name=format_name, depth=8, animation="AllFrames")
            cases.append((f"composite-export-{name}-{extension}", request, "render"))
    results = []
    for name, request, expected in cases:
        result = run_request(args.exe.resolve(), request, 40)
        reply = result.pop("_full_reply", result.get("reply", {}))
        good = result["status"] == "pass" and not reply.get("error")
        if expected == "password":
            good = result["status"] == "fail" and bool(reply.get("error")) and bool(reply.get("password_required"))
        elif expected == "list":
            good &= {"layered.psd", "layered.psb"} <= {item.get("name") for item in (reply.get("entries") or [])}
        elif expected == "render":
            rendered = reply.get("rendered") or {}
            good &= (rendered.get("width"), rendered.get("height"), rendered.get("frames")) == (2, 1, 1)
        else:
            good &= (reply.get("width"), reply.get("height"), reply.get("page_total")) == (2, 1, 1)
            good &= "文档合成预览" in reply.get("note", "")
            if expected == "thumbnail":
                good &= (reply.get("source_width"), reply.get("source_height")) == (2, 1)
                peak = result["memory"]["peak_bytes"]
                good &= peak is None or peak <= 576 * 1024 * 1024
        result.update(case=name, status="pass" if good else "fail")
        results.append(result)
        print(json.dumps({"case": name, "status": result["status"], "peak_bytes": result["memory"]["peak_bytes"]}), flush=True)
    unchanged = all(digest(path) == before for path, before in originals.items())
    report = {"passed": sum(r["status"] == "pass" for r in results),
              "failed": sum(r["status"] != "pass" for r in results),
              "sources_unchanged": unchanged, "cases": results,
              "scope": "Real Load/Thumbnail/archive/password/export supervisor paths; pixel fidelity and budget rejection are unit-tested"}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return bool(report["failed"] or not unchanged)


if __name__ == "__main__":
    raise SystemExit(main())
