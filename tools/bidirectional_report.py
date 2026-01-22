#!/usr/bin/env python3
"""
Generate a bidirectional (LaTeX->Typst + Typst->LaTeX) loss report.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def load_json(path: str) -> dict:
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def fmt_metrics(metrics: dict) -> str:
    return (
        f"papers/templates: {metrics.get('papers', metrics.get('templates', 0))}, "
        f"zero: {metrics.get('zero', 0)}, "
        f"<=5: {metrics.get('lte_5', 0)}, "
        f"<=10: {metrics.get('lte_10', 0)}, "
        f"avg: {metrics.get('avg', 0):.2f}, "
        f"max: {metrics.get('max', 0)}"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate bidirectional loss report.")
    parser.add_argument("--out", default="docs/bidirectional_loss_report.md")
    parser.add_argument(
        "--l2t",
        nargs="*",
        default=[],
        help="L2T summary.json paths",
    )
    parser.add_argument(
        "--t2l",
        nargs="*",
        default=[],
        help="T2L summary.json paths",
    )
    parser.add_argument("--title", default="Bidirectional Loss Report")
    parser.add_argument("--date", default="2026-01-18")
    args = parser.parse_args()

    lines: list[str] = []
    lines.append(f"# {args.title}\n")
    lines.append(f"Date: {args.date}\n")

    if args.l2t:
        lines.append("## LaTeX → Typst (L2T)\n")
        for path in args.l2t:
            data = load_json(path)
            name = Path(path).parent.name
            lines.append(f"### {name}\n")
            metrics = data.get("metrics_l2t", {})
            lines.append(f"**Metrics:** {fmt_metrics(metrics)}\n")
            lines.append("**Top losses**\n")
            for row in data.get("losses_l2t", [])[:10]:
                examples = ", ".join(row.get("example_papers", []))
                lines.append(
                    f"- {row['loss']} — total {row['total']}, papers {row['papers']}; "
                    f"examples: {examples}"
                )
            lines.append("")
            lines.append("**Worst papers**\n")
            for row in data.get("worst_papers_l2t", [])[:8]:
                lines.append(f"- {row['id']} — {row['l2t_loss_count']} ({row['run_dir']})")
            lines.append("")

    if args.t2l:
        lines.append("## Typst → LaTeX (T2L IR)\n")
        for path in args.t2l:
            data = load_json(path)
            name = Path(path).parent.name
            lines.append(f"### {name}\n")
            metrics = data.get("metrics_t2l_ir", {})
            lines.append(f"**Metrics:** {fmt_metrics(metrics)}\n")
            lines.append("**Top losses**\n")
            for row in data.get("losses_t2l_ir", [])[:10]:
                examples = ", ".join(row.get("example_templates", []))
                lines.append(
                    f"- {row['loss']} — total {row['total']}, templates {row['papers']}; "
                    f"examples: {examples}"
                )
            lines.append("")
            lines.append("**Worst templates**\n")
            for row in sorted(
                data.get("templates", []),
                key=lambda r: (-r.get("t2l_ir_loss_count", 0), r.get("id", "")),
            )[:8]:
                lines.append(
                    f"- {row['id']} — {row['t2l_ir_loss_count']} ({row['run_dir']})"
                )
            lines.append("")

    out_path = Path(args.out)
    out_path.write_text("\n".join(lines), encoding="utf-8")
    print(out_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
