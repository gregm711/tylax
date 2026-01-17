# Repository Guidelines

## Project Structure & Module Organization
- `src/`: core library and CLI entrypoint (`src/bin/t2l.rs`).
- `crates/`: internal subcrates (`tylax-ir`, `tylax-typst-frontend`, `tylax-latex-backend`).
- `tests/`: integration test suites plus fixtures under `tests/fixtures/`.
- `web/`: Vite-based demo UI; WASM output goes to `web/src/pkg/`.
- `assets/`, `docs/`, `tools/`: shared assets, documentation, and tooling.
- `latex-corpus/`, `typst-corpus/`: sample documents/templates used for coverage and fixtures.

## Build, Test, and Development Commands
- `cargo build --release --features cli`: build the CLI binary.
- `cargo test --release`: run the full Rust test suite.
- `wasm-pack build --target web --out-dir web/src/pkg --features wasm --no-default-features`: build WASM artifacts.
- `cd web && npm run dev`: start the web UI locally (install deps first).
- `cd web && npm run build`: production build of the web UI.

## Coding Style & Naming Conventions
- Rust code follows standard conventions: 4-space indentation, `snake_case` modules/functions, `PascalCase` types, `SCREAMING_SNAKE_CASE` constants.
- Format and lint before committing: `cargo fmt` and `cargo clippy --all-features`.
- Keep conversion logic under `src/core/{latex2typst,typst2latex}` and feature flags under `src/features/`.

## Testing Guidelines
- Uses Rust’s built-in test framework; tests live in `tests/*_tests.rs` with `#[test] fn test_*`.
- Prefer adding coverage to integration tests and reuse fixtures in `tests/fixtures/`.
- Run targeted suites by name, e.g., `cargo test latex2typst` or `cargo test tikz`.
- No explicit coverage threshold, but new behavior should include tests.
- Template snapshots (Typst → LaTeX) can be refreshed with `UPDATE_GOLDEN=1 cargo test -q template_snapshots`.
- Thesis corpus harness: `./tools/thesis_corpus_compile.sh` (convert → typst compile → report).
  - Env: `THESIS_LIST`, `THESIS_SKIP` (comma-separated), `TYPST_TIMEOUT`, `T2L_BIN`, `TYPST_BIN`.
  - `timeout`/`gtimeout` (or Python timeout) is used to avoid hangs on large templates.

## Commit & Pull Request Guidelines
- Commit messages are short, imperative, sentence-case (e.g., “Improve Typst→LaTeX layout”).
- PRs should include: what changed, why, tests run, and linked issues.
- For web/UI changes, include screenshots or a short GIF.

## Generated Artifacts & Config
- Treat `web/src/pkg/` as generated output from `wasm-pack`; regenerate rather than hand-edit.
- Keep secrets and local paths out of commits; store configuration in documented files only.

## Conversion Notes
- LaTeX → Typst: `\definecolor` / `\colorlet` from the preamble are captured as `#let` color definitions.
- CLI post-processes image paths to resolve extensionless `image("path")` against files copied into the output folder.
