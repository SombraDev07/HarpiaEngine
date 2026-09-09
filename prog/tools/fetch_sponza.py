#!/usr/bin/env python3
"""Download Khronos glTF-Sample-Assets Sponza (Crytek, CC-BY) into assets/sponza/."""
from __future__ import annotations

import json
import sys
import urllib.request
from pathlib import Path

MIRRORS = [
    "https://cdn.jsdelivr.net/gh/KhronosGroup/glTF-Sample-Assets@main/Models/Sponza/",
    "https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Assets/main/Models/Sponza/",
]
ROOT = Path(__file__).resolve().parents[2] / "assets" / "sponza"


def fetch(rel: str) -> bytes:
    last = None
    for base in MIRRORS:
        url = base + rel
        print(f"get {url}")
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "HarpiaEngine-fetch"})
            with urllib.request.urlopen(req, timeout=120) as r:
                return r.read()
        except Exception as e:
            last = e
            print(f"  fail: {e}")
    raise RuntimeError(f"could not fetch {rel}: {last}")


def write(rel: str, data: bytes) -> None:
    path = ROOT / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    print(f"  wrote {path} ({len(data)} bytes)")


def main() -> int:
    ROOT.mkdir(parents=True, exist_ok=True)
    write("README.md", fetch("README.md"))
    try:
        write("LICENSE.md", fetch("LICENSE.md"))
    except Exception:
        pass
    gltf_bytes = fetch("glTF/Sponza.gltf")
    write("glTF/Sponza.gltf", gltf_bytes)
    doc = json.loads(gltf_bytes.decode("utf-8"))
    names = set()
    for buf in doc.get("buffers", []):
        uri = buf.get("uri")
        if uri and not uri.startswith("data:"):
            names.add("glTF/" + uri)
    for img in doc.get("images", []):
        uri = img.get("uri")
        if uri and not uri.startswith("data:"):
            names.add("glTF/" + uri)
    for rel in sorted(names):
        write(rel, fetch(rel))
    print(f"done. load {ROOT / 'glTF' / 'Sponza.gltf'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
