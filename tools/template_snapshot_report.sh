#!/usr/bin/env bash
set -uo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
SNAPSHOT_DIR="$ROOT/tests/fixtures/templates"
TEMPLATE_DIR="${TEMPLATE_DIR:-$ROOT/public/templates}"
OUT_ROOT="$ROOT/target/template_snapshot"
REPORT="$ROOT/target/template_snapshot_report.md"
SUMMARY="$OUT_ROOT/summary.txt"

mkdir -p "$OUT_ROOT"
: > "$REPORT"
: > "$SUMMARY"

if [[ ! -d "$SNAPSHOT_DIR" ]]; then
  echo "[error] missing snapshot dir: $SNAPSHOT_DIR" | tee -a "$SUMMARY"
  exit 2
fi

T2L_BIN="${T2L_BIN:-}"
if [[ -n "$T2L_BIN" ]]; then
  if [[ ! -x "$T2L_BIN" ]]; then
    echo "[error] T2L_BIN is not executable: $T2L_BIN" | tee -a "$SUMMARY"
    exit 2
  fi
  t2l_cmd=("$T2L_BIN")
else
  if [[ ! -x "$ROOT/target/debug/t2l" ]]; then
    echo "[info] building t2l (debug)" | tee -a "$SUMMARY"
    if ! (cd "$ROOT" && cargo build -q --bin t2l); then
      echo "[error] failed to build t2l" | tee -a "$SUMMARY"
      exit 2
    fi
  fi
  t2l_cmd=("$ROOT/target/debug/t2l")
fi

sanitize() {
  local line="$1"
  line="${line//$'\r'/}"
  line="${line//|/\\|}"
  if [[ ${#line} -gt 160 ]]; then
    line="${line:0:157}..."
  fi
  printf "%s" "$line"
}

total=0
pass=0
fail=0
missing=0

printf "# Template Snapshot Report\\n\\n" >> "$REPORT"
printf "Templates: `%s` → snapshots in `%s`\\n\\n" "$TEMPLATE_DIR" "$SNAPSHOT_DIR" >> "$REPORT"
printf "| Template | Status | Details | Repro |\\n" >> "$REPORT"
printf "| --- | --- | --- | --- |\\n" >> "$REPORT"

has_snapshots=0
for expected in "$SNAPSHOT_DIR"/*.tex; do
  if [[ ! -e "$expected" ]]; then
    continue
  fi
  has_snapshots=1
  name=$(basename "$expected" .tex)
  if [[ -n "${ONLY_TEMPLATE:-}" && "$name" != "$ONLY_TEMPLATE" ]]; then
    continue
  fi

  total=$((total + 1))
  input="$TEMPLATE_DIR/$name.typ"
  out_dir="$OUT_ROOT/$name"
  output_tex="$out_dir/$name.tex"
  diff_file="$out_dir/$name.diff"
  convert_log="$out_dir/convert.log"
  mkdir -p "$out_dir"

  repro_cmd="ONLY_TEMPLATE=$name ./tools/template_snapshot_report.sh"

  if [[ ! -f "$input" ]]; then
    missing=$((missing + 1))
    fail=$((fail + 1))
    detail="missing typst input: $input"
    printf "| %s | fail | %s | %s |\\n" "$name" "$(sanitize "$detail")" "$repro_cmd" >> "$REPORT"
    echo "[fail] $name missing input: $input" | tee -a "$SUMMARY"
    continue
  fi

  if ! "${t2l_cmd[@]}" --direction t2l -f --ir -o "$output_tex" "$input" >"$convert_log" 2>&1; then
    fail=$((fail + 1))
    detail=$(head -n 1 "$convert_log")
    printf "| %s | fail | %s | %s |\\n" "$name" "$(sanitize "$detail")" "$repro_cmd" >> "$REPORT"
    echo "[fail] $name conversion failed" | tee -a "$SUMMARY"
    continue
  fi

  if diff -u "$expected" "$output_tex" >"$diff_file"; then
    pass=$((pass + 1))
    printf "| %s | pass | - | %s |\\n" "$name" "$repro_cmd" >> "$REPORT"
    echo "[ok] $name snapshot match" | tee -a "$SUMMARY"
  else
    fail=$((fail + 1))
    detail=$(head -n 1 "$diff_file")
    printf "| %s | fail | %s (see %s) | %s |\\n" "$name" "$(sanitize "$detail")" "$diff_file" "$repro_cmd" >> "$REPORT"
    echo "[fail] $name snapshot mismatch" | tee -a "$SUMMARY"
  fi
done

if [[ $has_snapshots -eq 0 ]]; then
  echo "[error] no snapshot files found in $SNAPSHOT_DIR" | tee -a "$SUMMARY"
  printf "| (none) | fail | no snapshots found | - |\\n" >> "$REPORT"
  exit 2
fi

pct() {
  local num="$1"
  local den="$2"
  awk -v n="$num" -v d="$den" 'BEGIN { if (d <= 0) { printf "0.0%%" } else { printf "%.1f%%", (n / d) * 100 } }'
}

printf "\\n## Summary\\n\\n" >> "$REPORT"
printf "| Metric | Count | Percent |\\n" >> "$REPORT"
printf "| --- | ---: | ---: |\\n" >> "$REPORT"
printf "| Total | %s | %s |\\n" "$total" "$(pct "$total" "$total")" >> "$REPORT"
printf "| Pass | %s | %s |\\n" "$pass" "$(pct "$pass" "$total")" >> "$REPORT"
printf "| Fail | %s | %s |\\n" "$fail" "$(pct "$fail" "$total")" >> "$REPORT"
printf "| Missing inputs | %s | %s |\\n" "$missing" "$(pct "$missing" "$total")" >> "$REPORT"

echo "Summary written to $SUMMARY"
echo "Report written to $REPORT"

if [[ $fail -ne 0 ]]; then
  exit 1
fi
