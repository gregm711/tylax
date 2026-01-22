#!/usr/bin/env python3
"""
Run the Tylax Typst -> LaTeX IR pipeline on a Typst corpus and rank losses.
"""

from __future__ import annotations

import argparse
import csv
import json
import os
import subprocess
from pathlib import Path
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor, as_completed


PREFERRED_NAMES = [
    "main.typ",
    "template.typ",
    "paper.typ",
    "article.typ",
    "thesis.typ",
]


def find_entrypoint_typ(root: str, max_depth: int = 4) -> str | None:
    candidates: list[str] = []
    for dirpath, _, files in os.walk(root):
        depth = len(Path(dirpath).relative_to(root).parts)
        if depth > max_depth:
            continue
        for name in files:
            if not name.lower().endswith(".typ"):
                continue
            path = os.path.join(dirpath, name)
            candidates.append(path)

    if not candidates:
        return None

    lower_map = {os.path.basename(p).lower(): p for p in candidates}
    for preferred in PREFERRED_NAMES:
        if preferred in lower_map:
            return lower_map[preferred]

    # Fallback: largest file
    candidates.sort(key=lambda p: os.path.getsize(p), reverse=True)
    return candidates[0]


def collect_template_dirs(corpus_dir: str, roots: list[str]) -> list[str]:
    dirs: list[str] = []
    for root in roots:
        base = os.path.join(corpus_dir, root)
        if not os.path.isdir(base):
            continue
        for name in sorted(os.listdir(base)):
            if name.startswith("."):
                continue
            path = os.path.join(base, name)
            if os.path.isdir(path):
                dirs.append(path)

    if dirs:
        return dirs

    # Fallback: collect any directories containing .typ
    seen = set()
    for dirpath, _, files in os.walk(corpus_dir):
        if any(f.lower().endswith(".typ") for f in files):
            if dirpath not in seen:
                seen.add(dirpath)
    return sorted(seen)


def run_cmd(cmd: list[str], cwd: str, log_path: str, timeout: int) -> int:
    with open(log_path, "a", encoding="utf-8") as log:
        log.write("$ " + " ".join(cmd) + "\n")
        try:
            proc = subprocess.run(
                cmd,
                cwd=cwd,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                timeout=timeout,
            )
        except subprocess.TimeoutExpired:
            log.write("ERROR: timeout\n")
            return 124
        log.write(proc.stdout or "")
        log.write(f"\n[exit={proc.returncode}]\n")
        return proc.returncode


def parse_loss_report(path: str) -> Counter[str]:
    if not os.path.exists(path):
        return Counter()
    try:
        with open(path, "r", encoding="utf-8") as f:
            data = json.load(f)
    except Exception:
        return Counter()
    losses = data.get("losses", [])
    counts: Counter[str] = Counter()
    for loss in losses:
        name = loss.get("name") or loss.get("kind") or "unknown"
        counts[str(name)] += 1
    return counts


def process_template(template_dir: str, args) -> dict:
    template_id = os.path.basename(template_dir.rstrip(os.sep))
    run_dir = os.path.abspath(os.path.join(args.out_dir, template_id))
    os.makedirs(run_dir, exist_ok=True)
    log_path = os.path.join(run_dir, "run.log")
    out_tex = os.path.join(run_dir, "out.tex")
    loss_log = os.path.join(run_dir, "t2l_ir_loss.json")

    if args.skip_existing and os.path.exists(loss_log):
        counts = parse_loss_report(loss_log)
        return {
            "id": template_id,
            "dir": template_dir,
            "entrypoint": None,
            "run_dir": run_dir,
            "loss_counts": counts,
            "exit_code": None,
            "skipped": False,
            "skip_reason": None,
        }

    entrypoint = find_entrypoint_typ(template_dir)
    if not entrypoint:
        with open(log_path, "a", encoding="utf-8") as log:
            log.write("ERROR: no .typ file found\n")
        return {
            "id": template_id,
            "dir": template_dir,
            "entrypoint": None,
            "run_dir": run_dir,
            "loss_counts": Counter(),
            "exit_code": None,
            "skipped": True,
            "skip_reason": "no_entrypoint",
        }

    entrypoint = str(Path(entrypoint).resolve())
    cmd = [
        os.path.abspath(args.t2l_bin),
        entrypoint,
        "--full-document",
        "--direction",
        "t2l",
        "--ir",
        "--output",
        out_tex,
        "--loss-log",
        loss_log,
    ]
    rc = run_cmd(cmd, template_dir, log_path, args.timeout)
    counts = parse_loss_report(loss_log)
    return {
        "id": template_id,
        "dir": template_dir,
        "entrypoint": entrypoint,
        "run_dir": run_dir,
        "loss_counts": counts,
        "exit_code": rc,
        "skipped": False,
        "skip_reason": None,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Run Typst -> LaTeX IR corpus.")
    parser.add_argument("--corpus-dir", default="typst-corpus")
    parser.add_argument(
        "--roots",
        default="typst-templates,ml-templates,package-templates-abs",
        help="Comma-separated subdirectories to treat as template roots.",
    )
    parser.add_argument("--out-dir", default="typst-corpus-runs")
    parser.add_argument("--t2l-bin", default="target/release/t2l")
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--skip-existing", action="store_true")
    parser.add_argument("--jobs", type=int, default=1)
    args = parser.parse_args()

    if not os.path.exists(args.t2l_bin):
        raise SystemExit(f"t2l binary not found: {args.t2l_bin}")

    args.out_dir = os.path.abspath(args.out_dir)
    os.makedirs(args.out_dir, exist_ok=True)

    roots = [r.strip() for r in args.roots.split(",") if r.strip()]
    template_dirs = collect_template_dirs(args.corpus_dir, roots)

    results = []
    if args.jobs <= 1:
        for template_dir in template_dirs:
            results.append(process_template(template_dir, args))
    else:
        with ThreadPoolExecutor(max_workers=args.jobs) as executor:
            futures = {
                executor.submit(process_template, d, args): d for d in template_dirs
            }
            for future in as_completed(futures):
                results.append(future.result())

    results.sort(key=lambda r: r["id"])

    total = Counter()
    papers_with: defaultdict[str, set[str]] = defaultdict(set)
    summary = {"templates": [], "losses_t2l_ir": []}

    for result in results:
        template_id = result["id"]
        counts = result["loss_counts"]
        if not result.get("skipped"):
            for k, v in counts.items():
                total[k] += v
                papers_with[k].add(template_id)
        summary["templates"].append(
            {
                "id": template_id,
                "dir": result["dir"],
                "entrypoint": result["entrypoint"],
                "run_dir": result["run_dir"],
                "t2l_ir_loss_count": sum(counts.values()),
                "t2l_exit": result["exit_code"],
                "skipped": result.get("skipped", False),
                "skip_reason": result.get("skip_reason"),
            }
        )

    def build_rank(counter: Counter[str], papers_map: dict[str, set[str]]):
        items = []
        for key, total in counter.items():
            papers = sorted(papers_map.get(key, set()))
            items.append(
                {
                    "loss": key,
                    "total": total,
                    "papers": len(papers),
                    "example_templates": papers[:5],
                }
            )
        items.sort(key=lambda x: (-x["papers"], -x["total"], x["loss"]))
        return items

    summary["losses_t2l_ir"] = build_rank(total, papers_with)

    counts = [
        t["t2l_ir_loss_count"]
        for t in summary["templates"]
        if not t.get("skipped")
    ]
    if counts:
        summary["metrics_t2l_ir"] = {
            "templates": len(counts),
            "zero": sum(1 for c in counts if c == 0),
            "lte_5": sum(1 for c in counts if c <= 5),
            "lte_10": sum(1 for c in counts if c <= 10),
            "avg": sum(counts) / len(counts),
            "max": max(counts),
        }
    else:
        summary["metrics_t2l_ir"] = {
            "templates": 0,
            "zero": 0,
            "lte_5": 0,
            "lte_10": 0,
            "avg": 0.0,
            "max": 0,
        }

    summary_path = os.path.join(args.out_dir, "summary.json")
    with open(summary_path, "w", encoding="utf-8") as f:
        json.dump(summary, f, indent=2)

    csv_path = os.path.join(args.out_dir, "summary.csv")
    with open(csv_path, "w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(["loss", "total", "papers"])
        for row in summary["losses_t2l_ir"]:
            writer.writerow([row["loss"], row["total"], row["papers"]])

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
