#!/usr/bin/env python3
"""Copy the Tauri installer to a stable GitHub Release name and write a SHA-256 sidecar."""

from __future__ import annotations

import argparse
import hashlib
import sys
import tempfile
from pathlib import Path

INSTALLER_SUFFIXES = {".dmg", ".msi", ".appimage"}


def canonical_suffix(path: Path) -> str:
    lowered = path.suffix.lower()
    if lowered == ".appimage":
        return ".AppImage"
    if lowered == ".msi":
        return ".msi"
    if lowered == ".dmg":
        return ".dmg"
    raise ValueError(f"unsupported installer suffix: {path}")


def find_installers(search_root: Path) -> list[Path]:
    found = [
        path
        for path in search_root.rglob("*")
        if path.is_file() and path.suffix.lower() in INSTALLER_SUFFIXES
    ]
    return sorted(found)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def stage(search_root: Path, output_dir: Path, target: str) -> Path:
    installers = find_installers(search_root)
    if len(installers) != 1:
        names = ", ".join(str(path) for path in installers) or "(none)"
        raise SystemExit(
            f"expected exactly one installer under {search_root}, found: {names}"
        )
    source = installers[0]
    output_dir.mkdir(parents=True, exist_ok=True)
    destination = output_dir / f"ccstats-desktop-{target}{canonical_suffix(source)}"
    destination.write_bytes(source.read_bytes())
    sidecar = output_dir / f"{destination.name}.sha256"
    sidecar.write_bytes(
        f"{sha256_file(destination)}  {destination.name}\n".encode("ascii")
    )
    print(destination)
    print(sidecar)
    return destination


def self_test() -> None:
    cases = [
        ("ccstats_0.5.1_aarch64.dmg", "aarch64-apple-darwin", ".dmg"),
        ("ccstats_0.5.1_x64_en-US.msi", "x86_64-pc-windows-msvc", ".msi"),
        ("ccstats_0.5.1_amd64.AppImage", "x86_64-unknown-linux-gnu", ".AppImage"),
    ]
    for filename, target, suffix in cases:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bundle = root / "bundle"
            bundle.mkdir()
            (bundle / filename).write_bytes(filename.encode("utf-8"))
            staged = stage(bundle, root / "staged", target)
            expected = root / "staged" / f"ccstats-desktop-{target}{suffix}"
            if staged != expected:
                raise SystemExit(f"staged path {staged} != {expected}")
            sidecar = expected.with_name(expected.name + ".sha256")
            digest = sha256_file(expected)
            expected_sidecar = f"{digest}  {expected.name}\n".encode("ascii")
            if sidecar.read_bytes() != expected_sidecar:
                raise SystemExit(f"checksum mismatch for {target}")
    print("stage-desktop-artifacts self-test passed")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--search-root", type=Path)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--target")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    if args.search_root is None or args.output_dir is None or args.target is None:
        parser.error("--search-root, --output-dir, and --target are required")
    stage(args.search_root, args.output_dir, args.target)
    return 0


if __name__ == "__main__":
    sys.exit(main())
