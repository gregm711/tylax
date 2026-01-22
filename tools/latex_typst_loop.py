#!/usr/bin/env python3
"""
Automated LaTeX -> Typst comparison loop for a project.

Steps:
1) Extract zip (if provided) to a temp dir.
2) Find a main .tex file (or use --main).
3) Compile LaTeX with tectonic.
4) Convert LaTeX -> Typst with t2l.
5) Compile Typst.
6) Render PDFs to PNGs and compute per-page diffs with ImageMagick compare.

Outputs a report JSON and diff images in a temp output directory.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from datetime import datetime, timezone


MAIN_CANDIDATES = (
    "main.tex",
    "paper.tex",
    "manuscript.tex",
    "index.tex",
)


def run(cmd, cwd=None, check=True):
    result = subprocess.run(
        cmd,
        cwd=cwd,
        capture_output=True,
        text=True,
    )
    if check and result.returncode != 0:
        raise RuntimeError(
            f"Command failed: {' '.join(cmd)}\n"
            f"cwd={cwd}\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    return result


def find_main_tex(project_dir: Path) -> Path:
    tex_files = sorted(project_dir.rglob("*.tex"))
    if not tex_files:
        raise RuntimeError(f"No .tex files found under {project_dir}")

    # Prefer standard names.
    for name in MAIN_CANDIDATES:
        for tex in tex_files:
            if tex.name == name:
                return tex

    # Otherwise pick a file containing \begin{document}.
    candidates = []
    for tex in tex_files:
        try:
            content = tex.read_text(encoding="utf-8", errors="ignore")
        except Exception:
            continue
        if "\\begin{document}" in content and "\\documentclass" in content:
            candidates.append(tex)

    if candidates:
        # Pick the shortest path as a heuristic.
        return sorted(candidates, key=lambda p: len(str(p)))[0]

    # Fallback: first file.
    return tex_files[0]


def extract_project(src: Path) -> Path:
    if src.is_dir():
        return src.resolve()
    if src.suffix.lower() != ".zip":
        raise RuntimeError(f"Unsupported input: {src}")

    tmp_dir = Path(tempfile.mkdtemp(prefix="tylax_project_", dir="/tmp"))
    shutil.unpack_archive(str(src), str(tmp_dir))

    # If archive contains a single top-level folder, use it.
    entries = [p for p in tmp_dir.iterdir() if p.is_dir()]
    if len(entries) == 1:
        return entries[0].resolve()
    return tmp_dir.resolve()


def parse_rmse(stderr: str) -> float | None:
    # compare -metric RMSE prints: "123.45 (0.00123)"
    match = re.search(r"([0-9]+\.?[0-9]*)\s*\(", stderr)
    if not match:
        return None
    try:
        return float(match.group(1))
    except ValueError:
        return None


def main() -> int:
    parser = argparse.ArgumentParser(description="LaTeX -> Typst diff loop")
    parser.add_argument("--project", required=False, help="Path to .zip or project dir")
    parser.add_argument("--main", help="Main .tex file (relative to project root)")
    parser.add_argument("--out", help="Output directory (default: temp dir)")
    parser.add_argument("--dpi", type=int, default=144, help="Render DPI for PDF -> PNG")
    parser.add_argument("--no-diff-images", action="store_true", help="Skip diff PNGs")
    parser.add_argument("--t2l-bin", help="Path to t2l binary (defaults to cargo run)")
    parser.add_argument("--tectonic-bin", default="tectonic", help="Tectonic binary")
    parser.add_argument("--typst-bin", default="typst", help="Typst binary")
    parser.add_argument("--pdftoppm-bin", default="pdftoppm", help="pdftoppm binary")
    parser.add_argument("--compare-bin", default="compare", help="ImageMagick compare binary")
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[1]

    project_arg = args.project
    if not project_arg:
        default_zip = repo_root / "OCR & GEPA.zip"
        if default_zip.exists():
            project_arg = str(default_zip)
        else:
            raise RuntimeError("No --project provided and default OCR & GEPA.zip not found.")

    project_src = Path(project_arg).resolve()
    project_dir = extract_project(project_src)

    if args.main:
        main_tex = (project_dir / args.main).resolve()
    else:
        main_tex = find_main_tex(project_dir)

    out_dir = Path(args.out).resolve() if args.out else Path(
        tempfile.mkdtemp(
            prefix="tylax_diff_",
            dir="/tmp",
        )
    )
    out_dir.mkdir(parents=True, exist_ok=True)

    latex_pdf = out_dir / "latex.pdf"
    typst_pdf = out_dir / "typst.pdf"
    typst_src = out_dir / "main.typ"

    # Compile LaTeX with tectonic.
    run(
        [
            args.tectonic_bin,
            "-X",
            "compile",
            "--outdir",
            str(out_dir),
            str(main_tex.name),
        ],
        cwd=project_dir,
        check=True,
    )

    # Find the produced PDF (tectonic uses input name by default).
    produced_pdf = out_dir / f"{main_tex.stem}.pdf"
    if produced_pdf.exists():
        produced_pdf.replace(latex_pdf)
    elif not latex_pdf.exists():
        raise RuntimeError(f"Expected LaTeX PDF not found in {out_dir}")

    # Convert LaTeX -> Typst.
    if args.t2l_bin:
        t2l_cmd = [args.t2l_bin]
    else:
        t2l_cmd = [
            "cargo",
            "run",
            "--quiet",
            "--release",
            "--features",
            "cli",
            "--bin",
            "t2l",
            "--",
        ]

    t2l_cmd += [
        "-d",
        "l2t",
        "-f",
        "--output",
        str(typst_src),
        str(main_tex),
    ]

    run(t2l_cmd, cwd=repo_root, check=True)

    # Compile Typst.
    run(
        [
            args.typst_bin,
            "compile",
            str(typst_src),
            str(typst_pdf),
        ],
        cwd=out_dir,
        check=True,
    )

    # Render PDFs to PNGs.
    latex_png_dir = out_dir / "latex_pages"
    typst_png_dir = out_dir / "typst_pages"
    latex_png_dir.mkdir(exist_ok=True)
    typst_png_dir.mkdir(exist_ok=True)

    run(
        [
            args.pdftoppm_bin,
            "-png",
            "-r",
            str(args.dpi),
            str(latex_pdf),
            str(latex_png_dir / "page"),
        ],
        check=True,
    )
    run(
        [
            args.pdftoppm_bin,
            "-png",
            "-r",
            str(args.dpi),
            str(typst_pdf),
            str(typst_png_dir / "page"),
        ],
        check=True,
    )

    latex_pages = sorted(latex_png_dir.glob("page-*.png"))
    typst_pages = sorted(typst_png_dir.glob("page-*.png"))
    page_count = min(len(latex_pages), len(typst_pages))

    diff_dir = out_dir / "diff_pages"
    if not args.no_diff_images:
        diff_dir.mkdir(exist_ok=True)

    results = []
    for i in range(page_count):
        latex_img = latex_pages[i]
        typst_img = typst_pages[i]
        diff_img = diff_dir / f"diff-{i+1:03d}.png"

        compare_cmd = [
            args.compare_bin,
            "-metric",
            "RMSE",
            str(latex_img),
            str(typst_img),
            str(diff_img if not args.no_diff_images else os.devnull),
        ]
        cmp = run(compare_cmd, check=False)
        rmse = parse_rmse(cmp.stderr)
        results.append(
            {
                "page": i + 1,
                "latex_image": str(latex_img),
                "typst_image": str(typst_img),
                "diff_image": None if args.no_diff_images else str(diff_img),
                "rmse": rmse,
                "compare_stderr": cmp.stderr.strip(),
            }
        )

    report = {
        "project": str(project_src),
        "project_dir": str(project_dir),
        "main_tex": str(main_tex),
        "out_dir": str(out_dir),
        "latex_pdf": str(latex_pdf),
        "typst_pdf": str(typst_pdf),
        "page_count": page_count,
        "dpi": args.dpi,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "results": results,
    }

    report_path = out_dir / "diff_report.json"
    report_path.write_text(json.dumps(report, indent=2))

    print("Diff report:", report_path)
    print("Output dir:", out_dir)
    print("LaTeX PDF:", latex_pdf)
    print("Typst PDF:", typst_pdf)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:
        print(f"Error: {exc}", file=sys.stderr)
        sys.exit(1)
