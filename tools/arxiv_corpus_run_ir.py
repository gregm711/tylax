#!/usr/bin/env python3
"""
Run the Tylax conversion pipeline on a local arXiv LaTeX corpus and rank losses.

Pipeline per paper:
  1) LaTeX -> Typst (full document) + loss report
  2) Typst -> LaTeX using IR + loss report

Outputs:
  - Per-paper outputs + logs
  - Summary JSON/CSV ranking loss kinds by prevalence
"""

from __future__ import annotations

import argparse
import csv
import json
import os
import re
import subprocess
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor, as_completed

DOCCLASS_RE = re.compile(r"\\documentclass(?:\[[^\]]*\])?\{([^}]+)\}")
BEGIN_DOC_RE = re.compile(r"\\begin\{document\}")


def read_file_head(path: str, limit: int = 1024 * 1024) -> str:
    try:
        with open(path, "r", encoding="utf-8", errors="ignore") as f:
            return f.read(limit)
    except OSError:
        return ""


def find_main_tex(paper_dir: str) -> str | None:
    best = None
    best_score = -1
    candidates = []
    for root, _, files in os.walk(paper_dir):
        for name in files:
            if not name.lower().endswith(".tex"):
                continue
            path = os.path.join(root, name)
            text = read_file_head(path)
            if not text:
                continue
            score = 0
            has_docclass = bool(DOCCLASS_RE.search(text))
            has_begin = bool(BEGIN_DOC_RE.search(text))
            if has_docclass:
                score += 5
            if has_begin:
                score += 3
            score += min(os.path.getsize(path) // 1024, 100)  # size proxy
            candidates.append((has_docclass, has_begin, score, path))

    if not candidates:
        return None

    # Prefer files that actually contain a documentclass. If none, fall back to
    # files with \\begin{document}. Only if neither exists, use the largest file.
    with_docclass = [c for c in candidates if c[0]]
    with_begin = [c for c in candidates if c[1]]

    pool = with_docclass or with_begin or candidates
    for _has_docclass, _has_begin, score, path in pool:
        if score > best_score:
            best_score = score
            best = path
    return best


def load_selected_dirs(path: str | None, corpus_dir: str) -> list[str]:
    if not path:
        # default: all first-level dirs
        dirs = []
        for name in sorted(os.listdir(corpus_dir)):
            full = os.path.join(corpus_dir, name)
            if os.path.isdir(full):
                dirs.append(full)
        return dirs
    with open(path, "r", encoding="utf-8") as f:
        data = json.load(f)
    if "selected" in data:
        return [item["dir"] for item in data["selected"]]
    if "dirs" in data:
        return data["dirs"]
    raise ValueError("Unsupported selection JSON format.")


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


def process_paper(paper_dir: str, args) -> dict:
    paper_id = os.path.basename(paper_dir.rstrip(os.sep))
    run_dir = os.path.abspath(os.path.join(args.out_dir, paper_id))
    os.makedirs(run_dir, exist_ok=True)
    log_path = os.path.join(run_dir, "run.log")
    typst_out = os.path.join(run_dir, "out.typ")
    l2t_loss = os.path.join(run_dir, "l2t_loss.json")
    t2l_loss = os.path.join(run_dir, "t2l_ir_loss.json")
    latex_out = os.path.join(run_dir, "out.tex")

    skip_path = l2t_loss if args.l2t_only else t2l_loss
    if args.skip_existing and os.path.exists(skip_path):
        l2t_counts = parse_loss_report(l2t_loss)
        t2l_counts = Counter()
        if not args.l2t_only:
            t2l_counts = parse_loss_report(t2l_loss)
        return {
            "id": paper_id,
            "dir": paper_dir,
            "main_tex": None,
            "run_dir": run_dir,
            "l2t_counts": l2t_counts,
            "t2l_counts": t2l_counts,
            "l2t_exit": None,
            "t2l_exit": None,
            "skipped": False,
            "skip_reason": None,
        }

    main_tex = find_main_tex(paper_dir)
    if not main_tex:
        with open(log_path, "a", encoding="utf-8") as log:
            log.write("ERROR: no .tex file found\n")
        return {
            "id": paper_id,
            "dir": paper_dir,
            "main_tex": None,
            "run_dir": run_dir,
            "l2t_counts": Counter(),
            "t2l_counts": Counter(),
            "l2t_exit": None,
            "t2l_exit": None,
            "skipped": True,
            "skip_reason": "no_entrypoint",
        }

    rel_tex = os.path.relpath(main_tex, paper_dir)

    # LaTeX -> Typst
    cmd_l2t = [
        os.path.abspath(args.t2l_bin),
        rel_tex,
        "--full-document",
        "--direction",
        "l2t",
        "--output",
        typst_out,
        "--loss-log",
        l2t_loss,
    ]
    rc1 = run_cmd(cmd_l2t, paper_dir, log_path, args.timeout)

    # Typst -> LaTeX (IR)
    rc2 = 0
    if (not args.l2t_only) and rc1 == 0 and os.path.exists(typst_out):
        cmd_t2l = [
            os.path.abspath(args.t2l_bin),
            typst_out,
            "--full-document",
            "--direction",
            "t2l",
            "--ir",
            "--output",
            latex_out,
            "--loss-log",
            t2l_loss,
        ]
        rc2 = run_cmd(cmd_t2l, paper_dir, log_path, args.timeout)

    l2t_counts = parse_loss_report(l2t_loss)
    t2l_counts = Counter()
    if not args.l2t_only:
        t2l_counts = parse_loss_report(t2l_loss)

    return {
        "id": paper_id,
        "dir": paper_dir,
        "main_tex": main_tex,
        "run_dir": run_dir,
        "l2t_counts": l2t_counts,
        "t2l_counts": t2l_counts,
        "l2t_exit": rc1,
        "t2l_exit": rc2,
        "skipped": False,
        "skip_reason": None,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Run IR pipeline on arXiv corpus.")
    parser.add_argument("--corpus-dir", default="arxiv-corpus")
    parser.add_argument(
        "--selection",
        default="arxiv-corpus/coverage.json",
        help="JSON with selected dirs (from arxiv_corpus_select.py).",
    )
    parser.add_argument("--out-dir", default="arxiv-corpus-runs")
    parser.add_argument("--t2l-bin", default="target/release/t2l")
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--skip-existing", action="store_true")
    parser.add_argument(
        "--l2t-only",
        action="store_true",
        help="Only run LaTeX -> Typst (skip Typst -> LaTeX IR).",
    )
    parser.add_argument(
        "--jobs",
        type=int,
        default=1,
        help="Number of concurrent papers to process.",
    )
    args = parser.parse_args()

    if not os.path.exists(args.t2l_bin):
        raise SystemExit(f"t2l binary not found: {args.t2l_bin}")

    args.out_dir = os.path.abspath(args.out_dir)
    os.makedirs(args.out_dir, exist_ok=True)
    selected_dirs = load_selected_dirs(args.selection, args.corpus_dir)

    summary = {
        "papers": [],
        "losses_l2t": {},
        "losses_t2l_ir": {},
    }

    total_l2t: Counter[str] = Counter()
    total_t2l_ir: Counter[str] = Counter()
    papers_with_l2t: defaultdict[str, set[str]] = defaultdict(set)
    papers_with_t2l: defaultdict[str, set[str]] = defaultdict(set)

    results = []
    if args.jobs <= 1:
        for paper_dir in selected_dirs:
            results.append(process_paper(paper_dir, args))
    else:
        with ThreadPoolExecutor(max_workers=args.jobs) as executor:
            futures = {
                executor.submit(process_paper, paper_dir, args): paper_dir
                for paper_dir in selected_dirs
            }
            for future in as_completed(futures):
                results.append(future.result())

    results.sort(key=lambda r: r["id"])
    for result in results:
        paper_id = result["id"]
        l2t_counts = result["l2t_counts"]
        t2l_counts = result["t2l_counts"]
        if not result.get("skipped"):
            for k, v in l2t_counts.items():
                total_l2t[k] += v
                papers_with_l2t[k].add(paper_id)
            for k, v in t2l_counts.items():
                total_t2l_ir[k] += v
                papers_with_t2l[k].add(paper_id)
        summary["papers"].append(
            {
                "id": paper_id,
                "dir": result["dir"],
                "main_tex": result["main_tex"],
                "run_dir": result["run_dir"],
                "l2t_loss_count": sum(l2t_counts.values()),
                "t2l_ir_loss_count": sum(t2l_counts.values()),
                "l2t_exit": result["l2t_exit"],
                "t2l_exit": result["t2l_exit"],
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
                    "example_papers": papers[:5],
                }
            )
        items.sort(key=lambda x: (-x["papers"], -x["total"], x["loss"]))
        return items

    summary["losses_l2t"] = build_rank(total_l2t, papers_with_l2t)
    summary["losses_t2l_ir"] = build_rank(total_t2l_ir, papers_with_t2l)

    def build_metrics(field: str) -> dict:
        counts = [
            p.get(field, 0) or 0
            for p in summary["papers"]
            if not p.get("skipped")
        ]
        total = len(counts)
        if total == 0:
            return {
                "papers": 0,
                "zero": 0,
                "lte_5": 0,
                "lte_10": 0,
                "avg": 0.0,
                "max": 0,
            }
        zero = sum(1 for c in counts if c == 0)
        lte_5 = sum(1 for c in counts if c <= 5)
        lte_10 = sum(1 for c in counts if c <= 10)
        return {
            "papers": total,
            "zero": zero,
            "lte_5": lte_5,
            "lte_10": lte_10,
            "avg": sum(counts) / total,
            "max": max(counts),
        }

    summary["metrics_l2t"] = build_metrics("l2t_loss_count")
    summary["metrics_t2l_ir"] = build_metrics("t2l_ir_loss_count")

    def worst_papers(key: str) -> list[dict]:
        ranked = sorted(
            [
                {"id": p["id"], key: p.get(key, 0), "run_dir": p["run_dir"]}
                for p in summary["papers"]
            ],
            key=lambda x: (-x[key], x["id"]),
        )
        return [r for r in ranked if r[key] and r[key] > 0][:20]

    summary["worst_papers_l2t"] = worst_papers("l2t_loss_count")
    summary["worst_papers_t2l_ir"] = worst_papers("t2l_ir_loss_count")

    summary_path = os.path.join(args.out_dir, "summary.json")
    with open(summary_path, "w", encoding="utf-8") as f:
        json.dump(summary, f, indent=2)

    # CSV output
    csv_path = os.path.join(args.out_dir, "summary.csv")
    with open(csv_path, "w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(["direction", "loss", "total", "papers"])
        for row in summary["losses_l2t"]:
            writer.writerow(["l2t", row["loss"], row["total"], row["papers"]])
        for row in summary["losses_t2l_ir"]:
            writer.writerow(["t2l_ir", row["loss"], row["total"], row["papers"]])

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
