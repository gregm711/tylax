#!/usr/bin/env bash
set -uo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
LIST_DEFAULT="$ROOT/tools/thesis_corpus_list_extended.txt"
LIST_FALLBACK="$ROOT/tools/thesis_corpus_list.txt"
LIST="${THESIS_LIST:-$LIST_DEFAULT}"
if [[ ! -f "$LIST" ]]; then
  LIST="$LIST_FALLBACK"
fi
OUT_ROOT="$ROOT/target/thesis_corpus"

mkdir -p "$OUT_ROOT"

summary="$OUT_ROOT/summary.txt"
report="$OUT_ROOT/report.md"
: > "$summary"
: > "$report"

printf "| Template | Losses | Missing Images | Missing Bib |\\n" >> "$report"
printf "| --- | ---: | ---: | ---: |\\n" >> "$report"

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
    printf "| %s | - | - | - |\\n" "$name (missing source)" >> "$report"
    continue
  fi

  out_dir="$OUT_ROOT/$name"
  mkdir -p "$out_dir"

  src_dir="$(dirname "$src")"
  rsync -a --exclude='.git' "$src_dir/" "$out_dir/" >/dev/null 2>&1 || true

  tex_path="$out_dir/$(basename "$src")"
  typ_path="$out_dir/$name.typ"

  if ! cargo run -q --bin t2l -- --direction l2t -f "$tex_path" -o "$typ_path"; then
    echo "[fail] $name conversion" | tee -a "$summary"
    printf "| %s | fail | - | - |\\n" "$name" >> "$report"
    continue
  fi

  loss_count=$(rg -o "tylax:loss:L[0-9]+" "$typ_path" | wc -l | tr -d ' ')

  missing_images=0
  missing_bib=0

  if [[ -f "$typ_path" ]]; then
    while IFS= read -r img; do
      [[ -z "$img" ]] && continue
      if [[ ! -f "$out_dir/$img" ]]; then
        missing_images=$((missing_images + 1))
      fi
    done < <(rg -o 'image\("[^"]+"' "$typ_path" | sed -e 's/^image(\"//' -e 's/\"$//')

    while IFS= read -r bib; do
      [[ -z "$bib" ]] && continue
      if [[ ! -f "$out_dir/$bib" ]]; then
        missing_bib=$((missing_bib + 1))
      fi
    done < <(rg -o 'bibliography\("[^"]+"' "$typ_path" | sed -e 's/^bibliography(\"//' -e 's/\"$//')
  fi

  echo "[ok] $name loss_count=$loss_count missing_images=$missing_images missing_bib=$missing_bib" | tee -a "$summary"
  printf "| %s | %s | %s | %s |\\n" "$name" "$loss_count" "$missing_images" "$missing_bib" >> "$report"

done < "$LIST"

echo "Summary written to $summary"
echo "Report written to $report"
