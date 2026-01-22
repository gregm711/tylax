# arXiv Loss Report (Extended Targeted Corpus)

Date: 2026-01-17  
Corpus: `arxiv-corpus-extended/`  
Runs: `arxiv-corpus-extended-runs/` (LaTeX -> Typst only)

## How this corpus was built
Targeted queries to surface common LaTeX templates and packages:
- IEEEtran, acmart, llncs, revtex, beamer
- tikz, pgfplots
- algorithm2e, minted, biblatex

Download manifest: `arxiv-corpus-extended/manifest.jsonl`  
Download errors: `arxiv-corpus-extended/errors.jsonl`

## How to reproduce a paper
Per-paper outputs and logs live under:
`arxiv-corpus-extended-runs/<paper_id>/`
- `run.log`
- `l2t_loss.json`
- `out.typ`

Re-run the batch:
```bash
python3 tools/arxiv_corpus_run_ir.py \
  --corpus-dir arxiv-corpus-extended \
  --selection arxiv-corpus-extended/coverage.json \
  --out-dir arxiv-corpus-extended-runs \
  --t2l-bin target/release/t2l \
  --timeout 60 \
  --l2t-only
```

## Top loss types (highest leverage)
No L2T losses reported in the current run.

## Worst papers (highest total loss counts)
No L2T losses reported in the current run.

## Notes
- This corpus is intentionally biased toward common LaTeX template pain points.
- If losses reappear, start with `summary.json`/`summary.csv` and inspect
  `l2t_loss.json` + `run.log` to find minimal repros.
