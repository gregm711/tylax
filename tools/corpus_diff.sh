#!/usr/bin/env bash
set -uo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
LIST="$ROOT/tools/corpus_list.txt"
OUT_ROOT="$ROOT/target/corpus_diff"

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

  tex_path="$out_dir/$name.tex"
  if ! latex=$(cargo run -q --bin t2l -- --direction t2l --ir -f "$src"); then
    echo "[fail] $name conversion" | tee -a "$summary"
    continue
  fi
  printf "%s" "$latex" > "$tex_path"

  if [[ $can_typst -eq 1 ]]; then
    if ! typst compile --root "$(dirname "$src")" "$src" "$out_dir/$name.typst.pdf" >/dev/null 2>&1; then
      echo "[fail] $name typst compile" | tee -a "$summary"
      continue
    fi
  else
    echo "[skip] $name typst compile (missing typst)" | tee -a "$summary"
    continue
  fi

  if [[ $can_tectonic -eq 1 ]]; then
    # Copy assets referenced by includegraphics.
    src_dir="$(dirname "$src")"
    assets=$(grep -oE '\\\\includegraphics(\\[[^]]*\\])?\\{[^}]+\\}' "$tex_path" | sed -E 's/.*\\{([^}]+)\\}.*/\\1/')
    for asset in $assets; do
      src_asset="$src_dir/$asset"
      dest="$out_dir/$asset"
      mkdir -p "$(dirname "$dest")"
      if [[ -f "$src_asset" ]]; then
        if [[ "$asset" == *.svg ]]; then
          if have_cmd magick; then
            magick "$src_asset" "${dest%.svg}.pdf" >/dev/null 2>&1 || true
            perl -pi -e 's/\.svg}/.pdf}/g' "$tex_path"
          else
            cp "$src_asset" "$dest"
          fi
        else
          cp "$src_asset" "$dest"
        fi
      else
        if [[ "$asset" == *.pdf ]]; then
          svg_src="${src_asset%.pdf}.svg"
          if [[ -f "$svg_src" ]] && have_cmd magick; then
            magick "$svg_src" "$dest" >/dev/null 2>&1 || true
          fi
        fi
      fi
    done
    if ! tectonic -X compile "$tex_path" --outdir "$out_dir" >/dev/null 2>&1; then
      echo "[fail] $name latex compile" | tee -a "$summary"
      continue
    fi
  else
    echo "[skip] $name latex compile (missing tectonic)" | tee -a "$summary"
    continue
  fi

  if [[ $can_pdftoppm -eq 1 && $can_compare -eq 1 ]]; then
    rm -rf "$out_dir/typst_png" "$out_dir/latex_png"
    mkdir -p "$out_dir/typst_png" "$out_dir/latex_png"

    pdftoppm -png "$out_dir/$name.typst.pdf" "$out_dir/typst_png/page" >/dev/null 2>&1
    pdftoppm -png "$out_dir/$name.pdf" "$out_dir/latex_png/page" >/dev/null 2>&1

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
