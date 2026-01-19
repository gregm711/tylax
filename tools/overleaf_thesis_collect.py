#!/usr/bin/env python3
"""
Collect Overleaf thesis templates into a local corpus.

This script scrapes Overleaf gallery tag pages (e.g., thesis, masters-thesis),
downloads the template page HTML, extracts the "View Source" code block,
and writes it to a local folder. It also stubs missing \\input/\\include and
bibliography files so conversion doesn't fail on missing files.
"""

from __future__ import annotations

import argparse
import html as html_mod
import json
import os
import re
import time
import urllib.parse
import urllib.request

BASE = "https://www.overleaf.com"
TEMPLATE_RE = re.compile(r"/latex/templates/([a-z0-9-]+)/([a-z0-9]+)")
PRE_CODE_RE = re.compile(r"<pre><code[^>]*>(.*?)</code></pre>", re.S)
MAINFILE_RE = re.compile(r"mainFile=([^&\"']+)")
INPUT_RE = re.compile(r"\\\\(?:input|include)\\{([^}]+)\\}")
BIB_RE = re.compile(r"\\\\(?:bibliography|addbibresource)\\{([^}]+)\\}")
ALGOLIA_APP = "SK53GL4JLY"
ALGOLIA_KEY = "9ac63d917afab223adbd2cd09ad0eb17"
ALGOLIA_INDEX = "gallery-production"


def fetch(url: str, timeout: int = 30) -> str:
    req = urllib.request.Request(
        url,
        headers={
            "User-Agent": "tylax-corpus-bot/1.0 (+https://www.overleaf.com)",
        },
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return resp.read().decode("utf-8", errors="ignore")


def extract_links(tag: str) -> list[str]:
    url = f"{BASE}/latex/templates/tagged/{tag}"
    html = fetch(url)
    links = []
    for slug, tid in TEMPLATE_RE.findall(html):
        if slug == "tagged":
            continue
        links.append(f"{BASE}/latex/templates/{slug}/{tid}")
    # Preserve order, drop duplicates.
    seen = set()
    out = []
    for link in links:
        if link in seen:
            continue
        seen.add(link)
        out.append(link)
    return out


def algolia_search(query: str, limit: int) -> list[str]:
    params = f"query={urllib.parse.quote(query)}&hitsPerPage={limit}"
    body = json.dumps({"params": params}).encode("utf-8")
    req = urllib.request.Request(
        f"https://{ALGOLIA_APP}-dsn.algolia.net/1/indexes/{ALGOLIA_INDEX}/query",
        data=body,
        headers={
            "X-Algolia-API-Key": ALGOLIA_KEY,
            "X-Algolia-Application-Id": ALGOLIA_APP,
            "Content-Type": "application/json",
            "User-Agent": "tylax-corpus-bot/1.0 (+https://www.overleaf.com)",
        },
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read().decode("utf-8", errors="ignore"))
    hits = data.get("hits", [])
    urls = []
    for hit in hits:
        if not hit.get("isTemplate") and hit.get("type") != "template":
            continue
        url = hit.get("url")
        if isinstance(url, str) and url.startswith("http"):
            urls.append(url)
    return urls


def ensure_file(path: str) -> None:
    if os.path.exists(path):
        return
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write("")


def stub_includes(base_dir: str, tex: str) -> None:
    for raw in INPUT_RE.findall(tex):
        name = raw.strip()
        if not name:
            continue
        if not name.lower().endswith(".tex"):
            name += ".tex"
        ensure_file(os.path.join(base_dir, name))
    for raw in BIB_RE.findall(tex):
        parts = [p.strip() for p in raw.split(",") if p.strip()]
        for part in parts:
            fname = part
            if not fname.lower().endswith(".bib"):
                fname += ".bib"
            ensure_file(os.path.join(base_dir, fname))


def main() -> int:
    parser = argparse.ArgumentParser(description="Collect Overleaf thesis templates.")
    parser.add_argument(
        "--tags",
        nargs="*",
        default=["thesis", "masters-thesis"],
        help="Gallery tags to scrape.",
    )
    parser.add_argument("--algolia-query", default="", help="Algolia query string (e.g., thesis).")
    parser.add_argument("--limit", type=int, default=40)
    parser.add_argument("--out-dir", default="latex-corpus/overleaf-thesis")
    parser.add_argument("--sleep", type=float, default=0.4)
    args = parser.parse_args()

    os.makedirs(args.out_dir, exist_ok=True)
    manifest_path = os.path.join(args.out_dir, "manifest.jsonl")

    links: list[str] = []
    if args.algolia_query:
        links.extend(algolia_search(args.algolia_query, args.limit))
    for tag in args.tags:
        links.extend(extract_links(tag))
    # Deduplicate across tags.
    seen = set()
    deduped = []
    for link in links:
        if link in seen:
            continue
        seen.add(link)
        deduped.append(link)
    links = deduped[: args.limit]

    manifest_lines = []
    dirs = []
    for idx, url in enumerate(links, 1):
        try:
            page = fetch(url)
        except Exception:
            continue
        m = PRE_CODE_RE.search(page)
        if not m:
            continue
        code_raw = html_mod.unescape(m.group(1))
        mainfile = "main.tex"
        mf = MAINFILE_RE.search(page)
        if mf:
            mainfile = urllib.parse.unquote(mf.group(1))
        # Normalize mainfile path.
        mainfile = mainfile.lstrip("/").replace("..", "")

        slug_id = url.rsplit("/", 1)[-1]
        slug = url.split("/latex/templates/")[-1].split("/")[0]
        dirname = f"{slug}-{slug_id}"
        dest_dir = os.path.join(args.out_dir, dirname)
        os.makedirs(dest_dir, exist_ok=True)

        dest_path = os.path.join(dest_dir, mainfile)
        os.makedirs(os.path.dirname(dest_path), exist_ok=True)
        with open(dest_path, "w", encoding="utf-8") as f:
            f.write(code_raw)

        stub_includes(dest_dir, code_raw)

        manifest_lines.append(
            json.dumps(
                {
                    "id": dirname,
                    "source": url,
                    "type": "overleaf",
                    "tags": args.tags,
                    "main": mainfile,
                }
            )
        )
        dirs.append(os.path.join(args.out_dir, dirname))
        if args.sleep:
            time.sleep(args.sleep)

    with open(manifest_path, "w", encoding="utf-8") as f:
        f.write("\n".join(manifest_lines) + ("\n" if manifest_lines else ""))

    coverage_path = os.path.join(args.out_dir, "coverage.json")
    with open(coverage_path, "w", encoding="utf-8") as f:
        json.dump({"dirs": dirs}, f, indent=2)
        f.write("\n")

    print(f"Collected {len(dirs)} templates into {args.out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
