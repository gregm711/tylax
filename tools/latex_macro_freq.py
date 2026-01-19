#!/usr/bin/env python3
"""Summarize LaTeX macro usage frequency across corpora.

Usage:
  python3 tools/latex_macro_freq.py --roots latex-corpus/overleaf-thesis arxiv-corpus \
    --top 50 --stoplist tools/latex_macro_stoplist.txt

Optionally include loss summaries to surface the most common unknown macros:
  --loss-summary path/to/summary.json [--loss-summary ...]
"""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter
from pathlib import Path


DEFAULT_STOPLIST = {
    "documentclass",
    "usepackage",
    "begin",
    "end",
    "document",
    "title",
    "author",
    "date",
    "maketitle",
    "section",
    "subsection",
    "subsubsection",
    "paragraph",
    "subparagraph",
    "chapter",
    "part",
    "label",
    "ref",
    "pageref",
    "cite",
    "citep",
    "citet",
    "citealt",
    "citealp",
    "bibliography",
    "bibliographystyle",
    "tableofcontents",
    "listoffigures",
    "listoftables",
    "include",
    "input",
    "includegraphics",
    "caption",
    "footnote",
    "emph",
    "textbf",
    "textit",
    "texttt",
    "textsc",
    "underline",
    "textsuperscript",
    "textsubscript",
    "item",
    "itemize",
    "enumerate",
    "description",
    "centering",
    "raggedright",
    "raggedleft",
    "hspace",
    "vspace",
    "smallskip",
    "medskip",
    "bigskip",
    "noindent",
    "newline",
    "linebreak",
    "pagebreak",
    "clearpage",
    "cleardoublepage",
    "pagenumbering",
    "thispagestyle",
    "pagestyle",
    "newpage",
    "bf",
    "it",
    "rm",
    "tt",
    "sc",
    "normalfont",
    "small",
    "large",
    "Large",
    "LARGE",
    "huge",
    "Huge",
    "normalsize",
    "tiny",
    # common math
    "frac",
    "sqrt",
    "sum",
    "prod",
    "int",
    "lim",
    "log",
    "ln",
    "exp",
    "sin",
    "cos",
    "tan",
    "alpha",
    "beta",
    "gamma",
    "delta",
    "epsilon",
    "theta",
    "lambda",
    "mu",
    "nu",
    "pi",
    "rho",
    "sigma",
    "tau",
    "phi",
    "omega",
    "left",
    "right",
    "mathrm",
    "mathbf",
    "mathcal",
    "mathbb",
    "cdot",
    "leq",
    "geq",
    "le",
    "ge",
    "infty",
    "hat",
    "bar",
}


def load_stoplist(path: str | None) -> set[str]:
    stop = set(DEFAULT_STOPLIST)
    if not path:
        return stop
    p = Path(path)
    if not p.exists():
        return stop
    for line in p.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        stop.add(line)
    return stop


def iter_tex_files(roots: list[str]) -> list[Path]:
    files: list[Path] = []
    for root in roots:
        r = Path(root)
        if not r.exists():
            continue
        files.extend(r.rglob("*.tex"))
    return files


def count_macros(paths: list[Path]) -> Counter[str]:
    macro_re = re.compile(r"\\\\?([A-Za-z@]+)")
    comment_re = re.compile(r"(?<!\\)%.*")
    counts: Counter[str] = Counter()
    for path in paths:
        try:
            text = path.read_text(errors="ignore")
        except Exception:
            continue
        lines = [comment_re.sub("", line) for line in text.splitlines()]
        text = "\n".join(lines)
        for m in macro_re.finditer(text):
            counts[m.group(1)] += 1
    return counts


def load_loss_summaries(paths: list[str]) -> list[tuple[str, int, int]]:
    losses: Counter[str] = Counter()
    papers: Counter[str] = Counter()
    for p in paths:
        path = Path(p)
        if not path.exists():
            continue
        data = json.loads(path.read_text())
        for loss in data.get("losses_l2t", []):
            name = loss.get("loss")
            total = int(loss.get("total", 0))
            paper_count = int(loss.get("papers", 0))
            if name:
                losses[name] += total
                papers[name] = max(papers[name], paper_count)
    return [(name, losses[name], papers[name]) for name in losses]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--roots", nargs="+", required=True)
    ap.add_argument("--top", type=int, default=30)
    ap.add_argument("--stoplist", default=None)
    ap.add_argument("--loss-summary", action="append", default=[])
    args = ap.parse_args()

    stop = load_stoplist(args.stoplist)
    paths = iter_tex_files(args.roots)
    counts = count_macros(paths)

    print(f"Scanned {len(paths)} .tex files")

    print("\nTop macros overall:")
    for name, count in counts.most_common(args.top):
        print(f"{name}: {count}")

    print("\nTop macros (excluding stoplist):")
    filtered = [(n, c) for n, c in counts.items() if n not in stop]
    for name, count in sorted(filtered, key=lambda x: x[1], reverse=True)[: args.top]:
        print(f"{name}: {count}")

    if args.loss_summary:
        print("\nTop unknown macros from loss summaries:")
        losses = load_loss_summaries(args.loss_summary)
        for name, total, papers in sorted(losses, key=lambda x: x[1], reverse=True)[: args.top]:
            print(f"{name}: {total} (papers {papers})")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
