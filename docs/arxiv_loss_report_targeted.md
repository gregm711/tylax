# arXiv Loss Report (Targeted Template/Package Corpus)

Date: 2026-01-17  
Corpus: `arxiv-corpus-targeted/`  
Runs: `arxiv-corpus-targeted-runs/` (LaTeX -> Typst only)

## How this corpus was built
Targeted queries to surface common templates and packages:
- Templates/classes: IEEEtran, acmart, llncs, revtex, beamer, elsarticle, svjour
- Venue macros: neurips, icml, iclr, cvpr, aaai, acl, emnlp
- Packages/features: tikz, pgfplots, algorithm2e, minted, biblatex, subcaption

Manifest: `arxiv-corpus-targeted/manifest.jsonl`  
Errors: `arxiv-corpus-targeted/errors.jsonl`

## How to reproduce a paper
Artifacts live under `arxiv-corpus-targeted-runs/<paper_id>/`:
- `run.log`
- `l2t_loss.json`
- `out.typ`

Batch re-run:
```bash
python3 tools/arxiv_corpus_run_ir.py \
  --corpus-dir arxiv-corpus-targeted \
  --selection arxiv-corpus-targeted/coverage.json \
  --out-dir arxiv-corpus-targeted-runs \
  --t2l-bin target/release/t2l \
  --timeout 60 \
  --l2t-only
```

## Highest‑leverage loss types
(Ordered by papers affected, then total count)
- xspace (152 total, 6 papers)  
  Examples: 1709.06005, 1801.08154, 2010.06000, 2103.02523, 2506.02738
- parse-error (17 total, 4 papers)  
  Examples: 1206.0287, 1801.08154, 2101.03700, 2506.02738
- ensuremath (42 total, 3 papers)  
  Examples: 1003.1919, 1107.3064, 1709.06005
- address (30 total, 3 papers)  
  Examples: 0811.2763, 1003.1919, 1206.0287
- setlength (5 total, 3 papers)  
  Examples: 1003.1919, 1709.06005, 2506.02738
- PACS (3 total, 3 papers)  
  Examples: 0811.2763, 1003.1919, 1107.3064
- fontsize (22 total, 2 papers)  
  Examples: 1709.06005, 2506.02738
- selectfont (22 total, 2 papers)  
  Examples: 1709.06005, 2506.02738
- linewidth (6 total, 2 papers)  
  Examples: 1801.08154, 2103.02523
- binom (5 total, 2 papers)  
  Examples: 1801.08154, 2503.08327
- abstract (2 total, 2 papers)  
  Examples: 0906.4191, 1107.3064
- keyword (2 total, 2 papers)  
  Examples: 0811.2763, 1003.1919

Notable high‑count single‑paper losses:
- norm (45 total) – 1206.0287
- “<” (21 total) – 1206.0287
- rotatebox (19 total) – 2506.02738

## Worst papers (highest total loss counts)
- 1206.0287 (127)
- 1801.08154 (102)
- 2506.02738 (100)
- 1709.06005 (67)
- 0811.2763 (52)
- 1107.3064 (46)
- 2510.00075 (36)
- 2010.06000 (28)
- 2103.02523 (21)
- 0910.1926 (18)

## Notes
- This corpus is biased toward real‑world template usage; it surfaces class and package
  macro gaps not obvious in generic samples.
- Start with xspace/ensuremath/address/PACS/parse-error for maximum coverage improvement.
