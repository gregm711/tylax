# arXiv Loss Report (Math/Refs/Tables/CJK/Chem Tranche)

Date: 2026-01-17  
Corpus: `arxiv-corpus-final/`  
Runs: `arxiv-corpus-final-runs/` (LaTeX -> Typst only)

## How this corpus was built
Focused queries for math, refs, tables, multilingual, and chemistry stacks:
- Math: amsart, amsbook, amsthm, mathtools, siunitx
- Refs: cleveref, natbib, biblatex
- Tables/figures: longtable, tabularx, booktabs, multirow, subcaption, wrapfig
- CJK/multilingual: ctex, babel, polyglossia, fontspec
- Chemistry: mhchem, chemfig
- Long-form: memoir, beamer

Manifest: `arxiv-corpus-final/manifest.jsonl`  
Errors: `arxiv-corpus-final/errors.jsonl`

## How to reproduce a paper
Artifacts live under `arxiv-corpus-final-runs/<paper_id>/`:
- `run.log`
- `l2t_loss.json`
- `out.typ`

Batch re-run:
```bash
python3 tools/arxiv_corpus_run_ir.py \
  --corpus-dir arxiv-corpus-final \
  --selection arxiv-corpus-final/coverage.json \
  --out-dir arxiv-corpus-final-runs \
  --t2l-bin target/release/t2l \
  --timeout 60 \
  --l2t-only
```

## Highest‑leverage loss types
This tranche surfaced a few large single‑paper gaps (physics/maths macro suites).
- bold (91 total, 1 paper)  
  Example: math_9908045
- Ket (80 total, 1 paper)  
  Example: 2308.07147
- ket (59 total, 1 paper)  
  Example: 2308.07147
- fontsize (19 total, 1 paper)  
  Example: 2506.02738
- rotatebox (19 total, 1 paper)  
  Example: 2506.02738
- selectfont (19 total, 1 paper)  
  Example: 2506.02738
- Bra (4 total, 1 paper)  
  Example: 2308.07147
- parse-error (3 total, 1 paper)  
  Example: 2310.05219
- smashoperator (2 total, 1 paper)  
  Example: 2506.02738

## Worst papers (highest total loss counts)
- 2308.07147 (144)
- math_9908045 (93)
- 2506.02738 (62)
- 0802.0480 (9)
- 2310.05219 (4)
- 2510.26564 (3)
- 2106.09696 (1)

## Notes
- This tranche is intentionally “spiky” (physics/math macro suites). Fixing those
  macros will unlock significant parity on older math/physics papers.
