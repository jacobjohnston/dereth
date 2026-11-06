#!/usr/bin/env python3
"""Draw the report library's places, retail beside current, and write a page to compare them.

    python3 tools/report_library.py                 every place
    python3 tools/report_library.py door clip       the places whose names hold these words

The places are in dereth/client/tests/gpu/report-library.txt. The page is written to
target/report-library/index.html. PACK names the art pack (the installed one by default) and
BEFORE a folder of an earlier run, drawn in a third column.
"""

import html
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LIBRARY = ROOT / "dereth/client/tests/gpu/report-library.txt"
OUT = ROOT / "target/report-library"
FEATURES = "retail-dats,vulkan"
TEST = "capture_the_frame_a_report_describes"


def places():
    """Each place's name, note, objects and report line."""
    doors, found, heading = "", [], None
    for line in LIBRARY.read_text().splitlines():
        if line.startswith("@doors "):
            doors = line[len("@doors "):]
        elif line.startswith("dereth report") and heading:
            name, note, objects = [part.strip() for part in heading.split("|")]
            found.append((name, note, doors if objects == "doors" else objects, line))
            heading = None
        elif line and not line.startswith("#"):
            heading = line
    return found


def test_binary():
    """The GPU tests' executable, copied so that a later build cannot change it during the run."""
    built = subprocess.run(
        ["cargo", "test", "--profile", "test-release", "-p", "dereth-client", "--features", FEATURES,
         "--test", "gpu", "--no-run", "--message-format=json"],
        cwd=ROOT, check=True, capture_output=True, text=True)
    path = None
    for line in built.stdout.splitlines():
        message = json.loads(line)
        if message.get("executable") and message.get("target", {}).get("name") == "gpu":
            path = message["executable"]
    if path is None:
        sys.exit("the GPU tests did not build")
    OUT.mkdir(parents=True, exist_ok=True)
    return shutil.copy2(path, OUT / "gpu-tests")


def draw(binary, name, objects, report):
    """Capture one place both ways; True when it drew."""
    folder = OUT / name
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "report.txt").write_text(report + "\n")
    env = dict(
        os.environ,
        DERETH_REPORT=str(folder / "report.txt"),
        DERETH_REPORT_OBJECTS=objects,
        DERETH_TEST_DAT_DIR=str(ROOT / "data"),
        DERETH_TEST_RENDERER="wgpu",
        DERETH_ART_PACK=os.environ.get("PACK", str(ROOT / "data/art-packs/enhanced-environment.dat")),
        DERETH_ART_PACK_WAYS="retail,pack",
        DERETH_HIFI_CAPTURE_DIR=str(folder),
    )
    result = subprocess.run([binary, TEST, "--ignored", "--nocapture"], cwd=ROOT / "dereth/client",
                            env=env, capture_output=True, text=True)
    if result.returncode != 0:
        print(f"{name}: did not draw\n{result.stdout[-400:]}{result.stderr[-400:]}")
        return False
    for way in ("retail", "pack"):
        subprocess.run(["sips", "-Z", "1100", "-s", "format", "jpeg", f"report-{way}.png", "--out", f"{way}.jpg"],
                       cwd=folder, capture_output=True, check=False)
    return True


def page(drawn):
    """The page comparing what was drawn."""
    before = os.environ.get("BEFORE")
    rows = []
    for name, note, _, _ in drawn:
        cells = [f'<figure><img loading="lazy" src="{name}/retail.jpg"><figcaption>Retail</figcaption></figure>']
        if before:
            cells.append(f'<figure><img loading="lazy" src="{before}/{name}/pack.jpg"><figcaption>Before</figcaption></figure>')
        cells.append(f'<figure><img loading="lazy" src="{name}/pack.jpg"><figcaption>Current</figcaption></figure>')
        rows.append(f"<section><h2>{html.escape(name)}</h2><p>{html.escape(note)}</p>"
                    f"<div>{''.join(cells)}</div></section>")
    (OUT / "index.html").write_text(
        "<!doctype html><meta charset=utf-8><title>Report library</title><style>"
        "body{font:15px system-ui;margin:24px;background:#16171a;color:#e8e8e8}"
        "section div{display:flex;gap:8px;flex-wrap:wrap}figure{margin:0;flex:1;min-width:300px}"
        "img{width:100%}figcaption{opacity:.7;padding:4px 0}</style>"
        "<h1>Report library</h1>" + "".join(rows))


def main():
    words = sys.argv[1:]
    chosen = [p for p in places() if not words or any(w in p[0] for w in words)]
    binary = test_binary()
    drawn = []
    for place in chosen:
        name, _, objects, report = place
        print(f"drawing {name}", flush=True)
        if draw(binary, name, objects, report):
            drawn.append(place)
    page(drawn)
    print(f"{len(drawn)} of {len(chosen)} drawn: {OUT / 'index.html'}")


if __name__ == "__main__":
    main()
