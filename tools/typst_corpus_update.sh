#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${1:-typst-corpus}"

REPOS=(
  "typst-templates|https://github.com/typst/templates.git"
  "ml-templates|https://github.com/daskol/typst-templates.git"
)

if [[ "${TYPST_CORPUS_SKIP_PACKAGES:-}" != "1" ]]; then
  REPOS+=("packages-src|https://github.com/typst/packages.git")
fi

for entry in "${REPOS[@]}"; do
  name="${entry%%|*}"
  url="${entry##*|}"
  dir="${ROOT_DIR}/${name}"

  if [[ -d "${dir}/.git" ]]; then
    echo "[update] ${dir}"
    (cd "${dir}" && git pull --ff-only) || {
      echo "[warn] ${dir} has local changes or diverged; skipping update."
    }
  else
    echo "[clone] ${url} -> ${dir}"
    git clone --depth 1 "${url}" "${dir}"
  fi
done
