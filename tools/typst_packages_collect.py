#!/usr/bin/env python3
"""
Collect Typst packages that look like templates/examples into a corpus folder.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path


ENTRYPOINT_NAMES = [
    "main.typ",
    "template.typ",
    "example.typ",
    "paper.typ",
    "article.typ",
    "thesis.typ",
]


def iter_package_dirs(packages_dir: Path) -> list[Path]:
    dirs: list[Path] = []
    for branch in ["preview", "release"]:
        root = packages_dir / "packages" / branch
        if not root.exists():
            root = packages_dir / branch
        if not root.is_dir():
            continue
        for pkg in root.iterdir():
            if not pkg.is_dir():
                continue
            for ver in pkg.iterdir():
                if ver.is_dir():
                    dirs.append(ver)
    return dirs


def has_template_like_file(path: Path, max_depth: int = 3) -> str | None:
    preferred = None
    any_typ = None
    for root, _, files in os.walk(path):
        depth = len(Path(root).relative_to(path).parts)
        if depth > max_depth:
            continue
        for name in files:
            if not name.lower().endswith(".typ"):
                continue
            file_path = Path(root) / name
            if name.lower() in ENTRYPOINT_NAMES:
                return str(file_path)
            if preferred is None and name.lower() in ["example.typ", "main.typ"]:
                preferred = str(file_path)
            if any_typ is None:
                any_typ = str(file_path)
    return preferred or any_typ


def main() -> int:
    parser = argparse.ArgumentParser(description="Collect Typst package templates.")
    parser.add_argument(
        "--packages-dir",
        default="",
        help="Path to typst/packages checkout (defaults to packages-src if present).",
    )
    parser.add_argument(
        "--out-dir",
        default="typst-corpus/package-templates-abs",
        help="Output directory to store package templates",
    )
    parser.add_argument(
        "--copy",
        action="store_true",
        help="Copy instead of symlink",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=200,
        help="Maximum number of package templates to collect.",
    )
    parser.add_argument(
        "--max-depth",
        type=int,
        default=3,
        help="Max directory depth to search for .typ files.",
    )
    args = parser.parse_args()

    packages_dir = Path(args.packages_dir) if args.packages_dir else None
    if packages_dir is None or not packages_dir.exists():
        fallback = Path("typst-corpus/packages-src")
        packages_dir = fallback if fallback.exists() else Path("typst-corpus/packages")

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    manifest_path = out_dir / "manifest.jsonl"
    records = []

    for pkg_dir in iter_package_dirs(packages_dir):
        entry = has_template_like_file(pkg_dir, max_depth=args.max_depth)
        if not entry:
            continue
        pkg = pkg_dir.parent.name
        ver = pkg_dir.name
        dest_name = f"{pkg}-{ver}"
        dest = out_dir / dest_name
        if dest.exists():
            continue

        if args.copy:
            # Copy only .typ files to keep it light.
            dest.mkdir(parents=True, exist_ok=True)
            for p in pkg_dir.glob("*.typ"):
                dest.joinpath(p.name).write_bytes(p.read_bytes())
        else:
            try:
                os.symlink(str(pkg_dir.resolve()), dest, target_is_directory=True)
            except OSError:
                # Fallback to copy
                dest.mkdir(parents=True, exist_ok=True)
                for p in pkg_dir.glob("*.typ"):
                    dest.joinpath(p.name).write_bytes(p.read_bytes())

        records.append(
            {
                "package": pkg,
                "version": ver,
                "dir": str(dest),
                "source": str(pkg_dir),
                "entrypoint": entry,
            }
        )
        if len(records) >= args.limit:
            break

    with open(manifest_path, "w", encoding="utf-8") as f:
        for record in records:
            f.write(json.dumps(record) + "\n")

    print(f"Collected {len(records)} package templates into {out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
