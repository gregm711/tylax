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
REPORT="$OUT_ROOT/compile_report.md"
SUMMARY="$OUT_ROOT/compile_summary.txt"

mkdir -p "$OUT_ROOT"
: > "$REPORT"
: > "$SUMMARY"

printf "| Template | Convert | Compile | First Error Line |\\n" >> "$REPORT"
printf "| --- | --- | --- | --- |\\n" >> "$REPORT"

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

TYPST_BIN="${TYPST_BIN:-typst}"
TYPST_TIMEOUT="${TYPST_TIMEOUT:-120}"
typst_available=1
if [[ -x "$TYPST_BIN" ]]; then
  typst_cmd="$TYPST_BIN"
elif command -v "$TYPST_BIN" >/dev/null 2>&1; then
  typst_cmd="$TYPST_BIN"
else
  typst_available=0
  typst_cmd="$TYPST_BIN"
fi

timeout_cmd=""
if command -v timeout >/dev/null 2>&1; then
  timeout_cmd="timeout"
elif command -v gtimeout >/dev/null 2>&1; then
  timeout_cmd="gtimeout"
fi

run_typst_compile() {
  local out_dir="$1"
  local typ_file="$2"
  local pdf_file="$3"
  if [[ -n "$timeout_cmd" ]]; then
    (cd "$out_dir" && "$timeout_cmd" "$TYPST_TIMEOUT" "$typst_cmd" compile "$typ_file" "$pdf_file")
    return $?
  fi
  if command -v python3 >/dev/null 2>&1; then
    python3 - <<PY
import subprocess, sys
try:
    subprocess.run(
        [${typst_cmd@Q}, "compile", ${typ_file@Q}, ${pdf_file@Q}],
        cwd=${out_dir@Q},
        check=True,
        timeout=float(${TYPST_TIMEOUT@Q}),
    )
    sys.exit(0)
except subprocess.TimeoutExpired:
    sys.exit(124)
except subprocess.CalledProcessError as e:
    sys.exit(e.returncode)
PY
    return $?
  fi
  (cd "$out_dir" && "$typst_cmd" compile "$typ_file" "$pdf_file")
  return $?
}

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
convert_pass=0
compile_pass=0
failures=0

while IFS= read -r line || [[ -n "$line" ]]; do
  [[ -z "$line" || "$line" =~ ^# ]] && continue
  name=$(echo "$line" | awk '{print $1}')
  path=$(echo "$line" | awk '{print $2}')
  if [[ -z "$name" || -z "$path" ]]; then
    continue
  fi

  if [[ -n "${THESIS_SKIP:-}" ]]; then
    IFS=',' read -r -a skip_list <<< "$THESIS_SKIP"
    for skip in "${skip_list[@]}"; do
      skip="${skip// /}"
      if [[ -n "$skip" && "$name" == "$skip" ]]; then
        printf "| %s | skip | skip | %s |\\n" "$name" "skipped via THESIS_SKIP" >> "$REPORT"
        echo "[skip] $name (THESIS_SKIP)" | tee -a "$SUMMARY"
        total=$((total + 1))
        continue 2
      fi
    done
  fi

  total=$((total + 1))
  src="$ROOT/$path"
  if [[ ! -f "$src" ]]; then
    error_line="missing source: $src"
    printf "| %s | fail | skip | %s |\\n" "$name (missing source)" "$(sanitize "$error_line")" >> "$REPORT"
    echo "[fail] $name missing source: $src" | tee -a "$SUMMARY"
    failures=$((failures + 1))
    continue
  fi

  out_dir="$OUT_ROOT/$name"
  mkdir -p "$out_dir"

  src_dir="$(dirname "$src")"
  rsync -a --exclude='.git' "$src_dir/" "$out_dir/" >/dev/null 2>&1 || true

  tex_path="$out_dir/$(basename "$src")"
  typ_path="$out_dir/$name.typ"
  convert_log="$out_dir/convert.log"
  compile_log="$out_dir/compile.log"

  convert_ok=0
  compile_ok=0
  error_line="-"

  if "${t2l_cmd[@]}" --direction l2t -f "$tex_path" -o "$typ_path" >"$convert_log" 2>&1; then
    convert_ok=1
    convert_pass=$((convert_pass + 1))
  else
    error_line=$(head -n 1 "$convert_log")
  fi

  if [[ $convert_ok -eq 1 ]]; then
    if [[ $typst_available -eq 1 ]]; then
      if run_typst_compile "$out_dir" "$name.typ" "$name.pdf" >"$compile_log" 2>&1; then
        compile_ok=1
        compile_pass=$((compile_pass + 1))
      else
        code=$?
        if [[ $code -eq 124 || $code -eq 137 ]]; then
          error_line="compile timeout (${TYPST_TIMEOUT}s)"
        else
          error_line=$(head -n 1 "$compile_log")
        fi
      fi
    else
      error_line="typst not found: $typst_cmd"
    fi
  fi

  convert_label="fail"
  compile_label="skip"
  if [[ $convert_ok -eq 1 ]]; then
    convert_label="pass"
    compile_label="fail"
  fi
  if [[ $compile_ok -eq 1 ]]; then
    compile_label="pass"
  fi

  if [[ $convert_ok -eq 1 && $compile_ok -eq 1 ]]; then
    echo "[ok] $name convert=pass compile=pass" | tee -a "$SUMMARY"
  else
    failures=$((failures + 1))
    echo "[fail] $name convert=$convert_label compile=$compile_label" | tee -a "$SUMMARY"
  fi

  printf "| %s | %s | %s | %s |\\n" "$name" "$convert_label" "$compile_label" "$(sanitize "$error_line")" >> "$REPORT"

done < "$LIST"

printf "\\n**Totals**\\n\\n" >> "$REPORT"
printf -- "- Total: %s\\n" "$total" >> "$REPORT"
printf -- "- Convert pass: %s\\n" "$convert_pass" >> "$REPORT"
printf -- "- Compile pass: %s\\n" "$compile_pass" >> "$REPORT"
printf -- "- Failures: %s\\n" "$failures" >> "$REPORT"

echo "Summary written to $SUMMARY"
echo "Report written to $REPORT"

if [[ $failures -ne 0 ]]; then
  exit 1
fi
