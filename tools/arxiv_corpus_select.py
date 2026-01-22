#!/usr/bin/env python3
"""
Select a diverse, high-coverage subset of arXiv LaTeX sources from a local corpus.

This script scans .tex files, extracts structural features (docclass, packages,
environments, and a few key commands), and then greedily selects papers that
maximize coverage of common features while keeping variety.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
from collections import Counter, defaultdict

DOCCLASS_RE = re.compile(r"\\documentclass(?:\[[^\]]*\])?\{([^}]+)\}")
USEPACKAGE_RE = re.compile(r"\\usepackage(?:\[[^\]]*\])?\{([^}]+)\}")
BEGIN_ENV_RE = re.compile(r"\\begin\{([^}]+)\}")

KEY_COMMANDS = [
    r"\\bibliography\b",
    r"\\addbibresource\b",
    r"\\printbibliography\b",
    r"\\newcommand\b",
    r"\\DeclareMathOperator\b",
    r"\\newtheorem\b",
    r"\\includegraphics\b",
    r"\\tikzpicture\b",
    r"\\begin\{tikzpicture\}",
    r"\\begin\{algorithm\}",
    r"\\begin\{algorithmic\}",
    r"\\begin\{lstlisting\}",
    r"\\begin\{minted\}",
]
KEY_COMMANDS_RE = [re.compile(p) for p in KEY_COMMANDS]


def strip_comments(text: str) -> str:
    lines = []
    for line in text.splitlines():
        out = []
        i = 0
        while i < len(line):
            ch = line[i]
            if ch == "%":
                # Count backslashes immediately before %
                bs = 0
                j = i - 1
                while j >= 0 and line[j] == "\\":
                    bs += 1
                    j -= 1
                if bs % 2 == 0:
                    break  # comment start
            out.append(ch)
            i += 1
        lines.append("".join(out))
    return "\n".join(lines)


def read_tex_files(paper_dir: str) -> list[str]:
    tex_files: list[str] = []
    for root, _, files in os.walk(paper_dir):
        for name in files:
            if name.lower().endswith(".tex"):
                tex_files.append(os.path.join(root, name))
    return tex_files


def extract_features_from_text(text: str) -> set[str]:
    features: set[str] = set()

    for m in DOCCLASS_RE.findall(text):
        for cls in m.split(","):
            cls = cls.strip()
            if cls:
                features.add(f"docclass:{cls}")

    for m in USEPACKAGE_RE.findall(text):
        for pkg in m.split(","):
            pkg = pkg.strip()
            if pkg:
                features.add(f"package:{pkg}")

    for env in BEGIN_ENV_RE.findall(text):
        env = env.strip()
        if env:
            features.add(f"env:{env}")

    for cre in KEY_COMMANDS_RE:
        if cre.search(text):
            features.add(f"cmd:{cre.pattern}")

    return features


def extract_features_for_paper(paper_dir: str) -> set[str]:
    features: set[str] = set()
    for path in read_tex_files(paper_dir):
        try:
            with open(path, "r", encoding="utf-8", errors="ignore") as f:
                text = f.read()
        except OSError:
            continue
        text = strip_comments(text)
        features |= extract_features_from_text(text)
    return features


def score_weight(freq: int, mode: str) -> float:
    if mode == "uniform":
        return 1.0
    if mode == "log":
        return math.log2(freq + 1.0)
    # "sqrt"
    return math.sqrt(freq)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Select a diverse, high-coverage subset of arXiv LaTeX sources."
    )
    parser.add_argument(
        "--corpus-dir",
        default="arxiv-corpus",
        help="Directory containing arXiv paper subdirectories",
    )
    parser.add_argument("--out", default="arxiv-coverage.json", help="Output JSON file")
    parser.add_argument("--top-features", type=int, default=200)
    parser.add_argument("--rare-features", type=int, default=50)
    parser.add_argument("--select", type=int, default=20)
    parser.add_argument(
        "--weight",
        default="log",
        choices=["uniform", "log", "sqrt"],
        help="Weighting for feature frequency",
    )
    parser.add_argument(
        "--min-feature-freq",
        type=int,
        default=2,
        help="Minimum frequency for features to be considered",
    )
    args = parser.parse_args()

    paper_dirs = []
    for name in sorted(os.listdir(args.corpus_dir)):
        path = os.path.join(args.corpus_dir, name)
        if os.path.isdir(path) and name not in ("src", "pkg"):
            paper_dirs.append(path)

    paper_features: dict[str, set[str]] = {}
    feature_freq: Counter[str] = Counter()

    for paper_dir in paper_dirs:
        feats = extract_features_for_paper(paper_dir)
        if not feats:
            continue
        paper_features[paper_dir] = feats
        feature_freq.update(feats)

    # Filter features and build target list
    filtered = {f: c for f, c in feature_freq.items() if c >= args.min_feature_freq}
    common = sorted(filtered.items(), key=lambda x: (-x[1], x[0]))[: args.top_features]
    rare = sorted(filtered.items(), key=lambda x: (x[1], x[0]))[: args.rare_features]

    target_features = {f for f, _ in common} | {f for f, _ in rare}
    target_weights = {f: score_weight(filtered[f], args.weight) for f in target_features}

    selected: list[dict] = []
    covered: set[str] = set()

    for _ in range(args.select):
        best_paper = None
        best_gain = 0.0
        best_new = set()
        for paper_dir, feats in paper_features.items():
            if any(p["dir"] == paper_dir for p in selected):
                continue
            new = feats & (target_features - covered)
            if not new:
                continue
            gain = sum(target_weights[f] for f in new)
            if gain > best_gain:
                best_gain = gain
                best_paper = paper_dir
                best_new = new
        if not best_paper:
            break
        covered |= best_new
        selected.append(
            {
                "dir": best_paper,
                "new_features": sorted(best_new),
                "new_feature_count": len(best_new),
                "total_features": len(paper_features[best_paper]),
            }
        )

    coverage = {
        "corpus_dir": args.corpus_dir,
        "papers_scanned": len(paper_features),
        "features_total": len(feature_freq),
        "features_considered": len(target_features),
        "selected_count": len(selected),
        "covered_count": len(covered),
        "coverage_ratio": (len(covered) / max(1, len(target_features))),
        "feature_frequencies_top": common[:50],
        "feature_frequencies_rare": rare[:50],
        "selected": selected,
    }

    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(coverage, f, indent=2)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
