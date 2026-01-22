#!/usr/bin/env python3
"""
Download arXiv source packages to build a local corpus for Tylax testing.

Notes:
- arXiv's API does not expose "most popular" or citation counts. This script
  uses arXiv's Atom API with relevance/date sorting. For truly "influential"
  papers, pass explicit IDs (e.g., from an external curated list).
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import tarfile
import time
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET

ARXIV_API = "http://export.arxiv.org/api/query"


def _http_get(url: str, timeout: int = 60) -> bytes:
    req = urllib.request.Request(
        url,
        headers={
            "User-Agent": "tylax-corpus-downloader/1.0 (+https://github.com/scipenai/tylax)",
        },
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return resp.read()


def fetch_arxiv_entries(
    query: str,
    start: int,
    max_results: int,
    sort_by: str,
    sort_order: str,
) -> list[dict]:
    params = {
        "search_query": query,
        "start": str(start),
        "max_results": str(max_results),
        "sortBy": sort_by,
        "sortOrder": sort_order,
    }
    url = ARXIV_API + "?" + urllib.parse.urlencode(params)
    data = _http_get(url)
    root = ET.fromstring(data)
    ns = {"atom": "http://www.w3.org/2005/Atom"}

    entries: list[dict] = []
    for entry in root.findall("atom:entry", ns):
        arxiv_id = entry.findtext("atom:id", default="", namespaces=ns).split("/abs/")[-1]
        title = entry.findtext("atom:title", default="", namespaces=ns).strip()
        published = entry.findtext("atom:published", default="", namespaces=ns)
        updated = entry.findtext("atom:updated", default="", namespaces=ns)
        summary = entry.findtext("atom:summary", default="", namespaces=ns).strip()
        authors = [
            a.findtext("atom:name", default="", namespaces=ns)
            for a in entry.findall("atom:author", ns)
        ]
        categories = [c.attrib.get("term") for c in entry.findall("atom:category", ns)]
        entries.append(
            {
                "id": arxiv_id,
                "title": title,
                "published": published,
                "updated": updated,
                "summary": summary,
                "authors": authors,
                "categories": categories,
                "source_url": f"https://arxiv.org/e-print/{arxiv_id}",
            }
        )
    return entries


def read_ids_list(ids_arg: str | None, ids_file: str | None) -> list[str]:
    ids: list[str] = []
    if ids_arg:
        ids.extend([x.strip() for x in ids_arg.split(",") if x.strip()])
    if ids_file:
        with open(ids_file, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#"):
                    continue
                ids.append(line)
    seen = set()
    out: list[str] = []
    for i in ids:
        if i in seen:
            continue
        seen.add(i)
        out.append(i)
    return out


def normalize_id(arxiv_id: str, strip_version: bool) -> tuple[str, str]:
    raw = arxiv_id
    if strip_version and "v" in arxiv_id:
        base = arxiv_id.split("v", 1)[0]
    else:
        base = arxiv_id
    safe = base.replace("/", "_")
    return raw, safe


def download_source(arxiv_id: str, dest_path: str) -> None:
    url = f"https://arxiv.org/e-print/{arxiv_id}"
    data = _http_get(url)
    tmp_path = dest_path + ".tmp"
    with open(tmp_path, "wb") as f:
        f.write(data)
    os.replace(tmp_path, dest_path)


def extract_if_tar(source_path: str, out_dir: str) -> bool:
    if not tarfile.is_tarfile(source_path):
        return False
    with tarfile.open(source_path) as tf:
        tf.extractall(out_dir)
    return True


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Download arXiv source packages for corpus testing."
    )
    parser.add_argument("--query", help="arXiv API query, e.g. 'cat:cs.CL'")
    parser.add_argument(
        "--max-results", type=int, default=25, help="Number of results to fetch"
    )
    parser.add_argument(
        "--sort-by",
        default="relevance",
        choices=["relevance", "lastUpdatedDate", "submittedDate"],
        help="arXiv API sort key",
    )
    parser.add_argument(
        "--sort-order",
        default="descending",
        choices=["ascending", "descending"],
        help="Sort order",
    )
    parser.add_argument(
        "--ids",
        help="Comma-separated arXiv IDs (overrides query fetch if provided)",
    )
    parser.add_argument(
        "--ids-file",
        help="Path to a file containing arXiv IDs (one per line)",
    )
    parser.add_argument(
        "--out-dir",
        default="arxiv-corpus",
        help="Output directory for downloaded sources",
    )
    parser.add_argument(
        "--sleep",
        type=float,
        default=1.0,
        help="Sleep seconds between downloads",
    )
    parser.add_argument(
        "--strip-version",
        action="store_true",
        help="Strip vN suffix from IDs for directory names",
    )
    parser.add_argument(
        "--skip-existing",
        action="store_true",
        help="Skip download if output directory already has a source file",
    )
    parser.add_argument(
        "--metadata-only",
        action="store_true",
        help="Only write manifest, do not download",
    )
    parser.add_argument(
        "--no-extract",
        action="store_true",
        help="Do not extract tarballs after download",
    )
    args = parser.parse_args()

    if not args.query and not args.ids and not args.ids_file:
        parser.error("Provide --query or --ids/--ids-file.")

    os.makedirs(args.out_dir, exist_ok=True)
    manifest_path = os.path.join(args.out_dir, "manifest.jsonl")
    errors_path = os.path.join(args.out_dir, "errors.jsonl")

    entries: list[dict] = []
    ids_list = read_ids_list(args.ids, args.ids_file)
    if ids_list:
        for arxiv_id in ids_list:
            entries.append(
                {
                    "id": arxiv_id,
                    "title": "",
                    "published": "",
                    "updated": "",
                    "summary": "",
                    "authors": [],
                    "categories": [],
                    "source_url": f"https://arxiv.org/e-print/{arxiv_id}",
                }
            )
    elif args.query:
        entries = fetch_arxiv_entries(
            args.query,
            start=0,
            max_results=args.max_results,
            sort_by=args.sort_by,
            sort_order=args.sort_order,
        )

    with open(manifest_path, "a", encoding="utf-8") as manifest, open(
        errors_path, "a", encoding="utf-8"
    ) as errors:
        for idx, entry in enumerate(entries):
            raw_id, safe_id = normalize_id(entry["id"], args.strip_version)
            paper_dir = os.path.join(args.out_dir, safe_id)
            os.makedirs(paper_dir, exist_ok=True)
            source_path = os.path.join(paper_dir, "source")
            extracted_dir = os.path.join(paper_dir, "src")

            entry_record = dict(entry)
            entry_record["id"] = raw_id
            entry_record["safe_id"] = safe_id
            entry_record["dir"] = paper_dir
            entry_record["source_path"] = source_path
            entry_record["extracted_dir"] = extracted_dir

            if args.metadata_only:
                manifest.write(json.dumps(entry_record) + "\n")
                manifest.flush()
                continue

            if args.skip_existing and os.path.exists(source_path):
                entry_record["downloaded"] = False
                entry_record["extracted"] = os.path.isdir(extracted_dir)
                manifest.write(json.dumps(entry_record) + "\n")
                manifest.flush()
                continue

            try:
                download_source(raw_id, source_path)
                entry_record["downloaded"] = True
                extracted = False
                if not args.no_extract:
                    os.makedirs(extracted_dir, exist_ok=True)
                    extracted = extract_if_tar(source_path, extracted_dir)
                entry_record["extracted"] = extracted
                manifest.write(json.dumps(entry_record) + "\n")
                manifest.flush()
            except Exception as exc:
                error_record = {
                    "id": raw_id,
                    "safe_id": safe_id,
                    "error": str(exc),
                }
                errors.write(json.dumps(error_record) + "\n")
                errors.flush()

            if idx < len(entries) - 1 and args.sleep > 0:
                time.sleep(args.sleep)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
