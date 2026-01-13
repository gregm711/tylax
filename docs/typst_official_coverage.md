# Typst Official Templates Coverage (Quick Pass)

Source scanned: `typst-official-templates/templates` (2026-01-13).

## High-level summary
- 8 templates discovered: appreciated-letter, badformer, cereal-words, charged-ieee, dashing-dept-news, icicle, unequivocal-ams, wonderous-book.
- **All 8** use `#show` rules in `template/main.typ`. (We now allow `#show heading` rules, but these templates use `#show: <template>.with(...)`.)
- **1 template** (charged-ieee) also uses `calc.*` and `{ ... }` code blocks in `template/main.typ`.
- Template `lib.typ` files are much more complex and use code blocks, set/show rules, functional collection methods, and calc.* widely.

## What this means for Tylax today
- Our converter parses only the input file, not remote packages. If you convert only `template/main.typ`, the heavy `lib.typ` usage won’t block parsing, but **we still ignore `#show` rules**, so styling/layout defined there is lost.
- `charged-ieee` is the exception where the main file itself uses `calc.*` and code blocks, which we still do not evaluate.

## Small-change path to higher coverage
- **Implement limited `#show heading.where(level: N)` support** (map to LaTeX heading styles via `titlesec`). This would cover most official templates that use show rules for headings and title blocks.
- **Allow a minimal subset of code blocks** for simple structural blocks (already partially done in the preprocessor) but extend for common template patterns (e.g., `block(...)`, `text(...)`, `v(...)`).
- **Template adapters** for a handful of official templates (charged-ieee, unequival-ams, wonderous-book) to translate show rules and metadata into LaTeX preamble settings.

## Observed main-file errors (from lint)
- `appreciated-letter`: show rules
- `badformer`: show rules
- `cereal-words`: show rules
- `charged-ieee`: show rules, code blocks, calc.*
- `dashing-dept-news`: show rules
- `icicle`: show rules
- `unequivocal-ams`: show rules
- `wonderous-book`: show rules

We now include lightweight adapters for `letter.with`, `book.with`, `ams-article.with`, `newsletter.with`, and `ieee.with`, which makes the official templates **renderable** (layout may still be simplified because we do not execute their libraries).
