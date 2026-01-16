#!/usr/bin/env bash
set -uo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
LIST="$ROOT/tools/latex_corpus_list.txt"
OUT_ROOT="$ROOT/target/latex_corpus_diff"

mkdir -p "$OUT_ROOT"

have_cmd() {
  command -v "$1" >/dev/null 2>&1
}

can_typst=0
can_tectonic=0
can_pdftoppm=0
can_compare=0

have_cmd typst && can_typst=1
have_cmd tectonic && can_tectonic=1
have_cmd pdftoppm && can_pdftoppm=1
have_cmd compare && can_compare=1

summary="$OUT_ROOT/summary.txt"
: > "$summary"

while IFS= read -r line; do
  [[ -z "$line" || "$line" =~ ^# ]] && continue
  name=$(echo "$line" | awk '{print $1}')
  path=$(echo "$line" | awk '{print $2}')
  if [[ -z "$name" || -z "$path" ]]; then
    continue
  fi
  src="$ROOT/$path"
  if [[ ! -f "$src" ]]; then
    echo "[skip] $name missing: $src" | tee -a "$summary"
    continue
  fi

  out_dir="$OUT_ROOT/$name"
  mkdir -p "$out_dir"

  src_dir="$(dirname "$src")"
  rsync -a --exclude='.git' "$src_dir/" "$out_dir/" >/dev/null 2>&1 || true

  tex_path="$out_dir/$(basename "$src")"
  typ_path="$out_dir/$name.typ"
  typ_clean="$out_dir/$name.clean.typ"

  # Sanitize BibTeX files for Typst (convert @String(...) -> @string{...})
  for bib in "$out_dir"/*.bib; do
    [[ -f "$bib" ]] || continue
    python3 - <<'PY' "$bib" || true
import re, sys
from pathlib import Path

path = Path(sys.argv[1])
text = path.read_text()
out = ""
i = 0
while True:
    m = re.search(r'@String\s*\(', text[i:], re.IGNORECASE)
    if not m:
        out += text[i:]
        break
    start = i + m.start()
    out += text[i:start] + "@string{"
    j = start + m.group(0).__len__()
    depth = 1
    k = j
    while k < len(text):
        if text[k] == "(":
            depth += 1
        elif text[k] == ")":
            depth -= 1
            if depth == 0:
                out += text[j:k] + "}"
                i = k + 1
                break
        k += 1
    else:
        out += text[j:]
        i = len(text)
        break

path.write_text(out)
# Sanitize entry keys to Typst-friendly form (letters, numbers, hyphens).
def sanitize_key(key: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9-]+", "-", key)
    cleaned = re.sub(r"-+", "-", cleaned).strip("-")
    return cleaned or key.strip()

def repl(match: re.Match) -> str:
    entry_type = match.group(1)
    key = match.group(2)
    return f"@{entry_type}{{{sanitize_key(key)},"

text = path.read_text()
text = re.sub(r"@([A-Za-z]+)\s*[\{\(]\s*([^,\s]+)\s*,", repl, text)
path.write_text(text)
PY
  done

  if ! cargo run -q --bin t2l -- --direction l2t -f "$src" -o "$typ_path"; then
    echo "[fail] $name conversion" | tee -a "$summary"
    continue
  fi

  if [[ $can_tectonic -eq 1 ]]; then
    if ! tectonic -X compile "$tex_path" --outdir "$out_dir" >/dev/null 2>&1; then
      echo "[fail] $name latex compile" | tee -a "$summary"
      continue
    fi
  else
    echo "[skip] $name latex compile (missing tectonic)" | tee -a "$summary"
    continue
  fi

  # Strip converter loss markers/comments for compilation.
  perl -0pe 's#/\*.*?\*/\s*##gs' "$typ_path" > "$typ_clean"
  perl -pi -e 's/#v\(\s*\)/#v(0pt)/g; s/#h\(\s*\)/#h(0pt)/g' "$typ_clean"

  if [[ $can_typst -eq 1 ]]; then
    if ! typst compile --root "$out_dir" "$typ_clean" "$out_dir/$name.typst.pdf" >/dev/null 2>&1; then
      echo "[fail] $name typst compile" | tee -a "$summary"
      continue
    fi
  else
    echo "[skip] $name typst compile (missing typst)" | tee -a "$summary"
    continue
  fi

  latex_pdf="$out_dir/$(basename "$tex_path" .tex).pdf"

  if [[ $can_pdftoppm -eq 1 && $can_compare -eq 1 ]]; then
    rm -rf "$out_dir/typst_png" "$out_dir/latex_png"
    mkdir -p "$out_dir/typst_png" "$out_dir/latex_png"

    pdftoppm -png "$out_dir/$name.typst.pdf" "$out_dir/typst_png/page" >/dev/null 2>&1
    pdftoppm -png "$latex_pdf" "$out_dir/latex_png/page" >/dev/null 2>&1

    rmse_max=0
    for png in "$out_dir/typst_png"/*.png; do
      base=$(basename "$png")
      other="$out_dir/latex_png/$base"
      diff="$out_dir/diff-$base"
      if [[ -f "$other" ]]; then
        metric=$(compare -metric RMSE "$png" "$other" "$diff" 2>&1 | tr -d '()' | awk '{print $2}')
        metric=${metric:-0}
        rmse_max=$(awk -v a="${rmse_max:-0}" -v b="$metric" 'BEGIN { if (b+0 > a+0) print b; else print a }')
      fi
    done
    echo "[ok] $name rmse=$rmse_max" | tee -a "$summary"
  else
    echo "[ok] $name compiled (missing pdftoppm/compare for diff)" | tee -a "$summary"
  fi

 done < "$LIST"

 echo "Summary written to $summary"
