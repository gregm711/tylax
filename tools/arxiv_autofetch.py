#!/usr/bin/env python3
"""
Auto-generate (and optionally execute) arXiv fetches based on loss reports.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
from pathlib import Path


def load_json(path: str) -> dict:
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def load_query_map(path: str | None) -> dict:
    if not path:
        return {}
    if not os.path.exists(path):
        return {}
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def is_safe_loss_name(name: str) -> bool:
    return bool(re.fullmatch(r"[A-Za-z0-9_\\-]+", name))


def build_queries(loss: str, query_map: dict) -> list[str]:
    if loss in query_map:
        return query_map[loss]
    if not is_safe_loss_name(loss):
        return []
    return [f"all:{loss}"]


def main() -> int:
    parser = argparse.ArgumentParser(description="Auto-fetch arXiv papers for new losses.")
    parser.add_argument(
        "--summaries",
        nargs="+",
        required=True,
        help="L2T summary.json paths",
    )
    parser.add_argument("--out-dir", default="arxiv-corpus-autofetch")
    parser.add_argument("--min-papers", type=int, default=3)
    parser.add_argument("--max-losses", type=int, default=15)
    parser.add_argument("--max-results", type=int, default=6)
    parser.add_argument("--query-map", default="tools/loss_query_map.json")
    parser.add_argument("--execute", action="store_true")
    parser.add_argument("--sleep", type=float, default=1.0)
    args = parser.parse_args()

    query_map = load_query_map(args.query_map)
    loss_scores = {}

    for path in args.summaries:
        data = load_json(path)
        for row in data.get("losses_l2t", []):
            if row.get("papers", 0) < args.min_papers:
                continue
            loss_scores.setdefault(row["loss"], 0)
            loss_scores[row["loss"]] += row.get("papers", 0)

    ranked = sorted(
        loss_scores.items(), key=lambda x: (-x[1], x[0])
    )[: args.max_losses]

    plan = []
    for loss, papers in ranked:
        for query in build_queries(loss, query_map):
            plan.append(
                {
                    "loss": loss,
                    "papers": papers,
                    "query": query,
                    "max_results": args.max_results,
                }
            )

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    plan_path = out_dir / "autofetch_plan.json"
    plan_path.write_text(json.dumps(plan, indent=2), encoding="utf-8")

    if not args.execute:
        print(plan_path)
        return 0

    for item in plan:
        cmd = [
            "python3",
            "tools/arxiv_corpus_download.py",
            "--query",
            item["query"],
            "--max-results",
            str(item["max_results"]),
            "--sort-by",
            "relevance",
            "--sort-order",
            "descending",
            "--out-dir",
            str(out_dir),
            "--skip-existing",
            "--strip-version",
            "--sleep",
            str(args.sleep),
        ]
        subprocess.run(cmd, check=False)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
