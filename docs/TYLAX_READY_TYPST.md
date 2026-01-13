# Tylax‑Ready Typst (Subset Spec)

This spec defines the **Typst subset** we support for high‑fidelity Typst → LaTeX conversion.
If templates stay inside this subset, we can guarantee **near‑perfect parity** and stable output.

## Goals
- High‑fidelity, deterministic conversion to LaTeX
- Stable output across templates
- Minimal evaluator (no full Typst runtime)

## Allowed (Core)
### Structure
- Headings: `=`, `==`, `===`, `====`
- Paragraphs, lists (`-`, `+`, numbered)
- Quotes, code blocks (markup `\`code\`` only, no `{ ... }` blocks)
- Math: `$...$` and block equations
- References: `#ref`, `#label`, `#cite`, footnotes, `#bibliography("refs.bib")`

### Layout & Blocks
- `#align(...)`
- `#block(...)`
- `#box(...)`
- `#columns(columns: n)[...]`
- `#grid(columns: n, gutter: ...)[...]`
- `#table(...)` (columns, align, caption, stroke, fill, inset)
- `#figure(...)`
- `#image(...)` (path + width/height/fit)
- `#outline(...)`

### Inline Formatting
- `*bold*`, `_italic_`, `` `code` ``
- `#text(...)` (limited: weight/style/fill; size ignored)

### Set Rules (Limited)
- `#set page(...)` (paper + margin only)
- `#set text(...)` (size + font only; size maps to docclass option when possible)
- `#set par(...)` (justify + leading + first-line-indent)
- `#set math.equation(numbering: "...")` (enables numbered equations)

## Allowed (Control Flow)
### Variables / Functions
- `#let name = <simple value>`
- `#let func(params...) = [content]`
  - **Content blocks only**, not `{ ... }` code blocks

### Conditionals / Loops
- `#if` with booleans or `==/!= none`
- `#for x in (a, b, c)` or `#for x in range(...)`
- `range(end)` or `range(start, end[, step])`

## Allowed Value Types
- `none`, `true/false`
- strings, numbers
- arrays / dictionaries (literal)
- color tokens as hex strings (e.g., `"#0ea5e9"`)
- spacing tokens as strings (e.g., `"8pt"`, `"0.5em"`)

## Disallowed (For Now)
- `#show` rules (except `#show heading` / `#show heading.where(level: ...)`)
- `#set` rules (other than page/text/par/math.equation)
- `{ ... }` code blocks (procedural Typst)
- `.map`, `.filter`, `.fold`, `.reduce`, `.join`
- `calc.*`
- `place(...)`
- spread `..` / `...`
- dynamic state / counters beyond simple `counter(...).step()/display()`

## Migration Patterns
| Disallowed | Replace With |
|---|---|
| `authors.map(author => [...])` | `#for author in authors [ ... ]` |
| `#show heading: ...` | explicit `#heading(...)` wrappers |
| `#place(...)` | `#align(...)` + `#block(...)` |
| `#set text(...)` | inline `#text(...)` or markdown emphasis |

## Linting
Use the linter to validate templates:
```
cargo run --bin typst_subset_lint -- ../public/templates
```

## Notes
This subset is intentionally minimal. It can be extended, but only with features that map
cleanly to LaTeX and do not require a full Typst evaluator.
