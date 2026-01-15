# AI Repair Hook (Tylax)

This repo supports a self-healing LaTeX -> Typst conversion loop via an external AI hook.

## Protocol

The repair hook is a command that:
- reads JSON from **stdin**
- writes **Typst source only** to **stdout**

The JSON payload looks like:

```
{
  "input": "<original LaTeX>",
  "output": "<deterministic Typst>",
  "report": {
    "source_lang": "latex",
    "target_lang": "typst",
    "losses": [
      {
        "id": "L0001",
        "kind": "unknown-command",
        "name": "unknowncmd",
        "message": "Unknown command \\unknowncmd",
        "snippet": "\\unknowncmd{a}",
        "context": "math"
      }
    ],
    "warnings": []
  },
  "metrics": {
    "headings": 1,
    "equations": 0,
    "figures": 0,
    "tables": 0,
    "cites": 0,
    "refs": 0,
    "labels": 0,
    "list_items": 0,
    "loss_markers": 1,
    "parse_errors": 0
  }
}
```

## Validation Gate

The output is accepted only if:
- no Typst parse errors
- no increase in subset lint issues
- structural metrics are >= baseline
- loss markers decrease (unless `--allow-no-gain`)

## Agno + OpenRouter Hook (LaTeX -> Typst)

```
pip install agno
export OPEN_ROUTER_API_KEY=...
export TYLAX_AI_CMD="python tools/ai_repair_agno.py"

cargo run --bin tylax_repair -- input.tex -o out.typ --full-document --auto-repair
```

Configurable env vars:
- `TYLAX_AI_MODEL` or `OPENROUTER_MODEL` (default: `x-ai/grok-4.1-fast`)
- `TYLAX_AI_REASONING` (default: 1)
- `OPENROUTER_SITE`, `OPENROUTER_APP_NAME`

## Agno + OpenRouter Hook (Typst -> LaTeX)

```
pip install agno
export OPEN_ROUTER_API_KEY=...
export TYLAX_AI_CMD="python tools/ai_repair_agno_t2l.py"

cargo run --bin t2l -- input.typ -o out.tex --direction t2l --ir --auto-repair
```

## Post-repair reports

You can write a second report after AI fixes to verify loss markers are gone:

```
cargo run --bin t2l -- input.tex -o out.typ \
  --full-document --auto-repair --loss-log /tmp/pre.json --post-repair-log /tmp/post.json
```

OpenRouter-only hook (no Agno):

```
export OPENROUTER_API_KEY=...
export TYLAX_AI_CMD="python tools/ai_repair_openrouter_t2l.py"
```

## Stub Hook (no keys required)

```
export TYLAX_AI_CMD="python tools/ai_repair_stub.py"
```

This stub simply strips `tylax:loss:` markers to exercise the repair loop.
