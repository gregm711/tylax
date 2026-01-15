#!/usr/bin/env python3
import asyncio
import json
import os
import re
import sys

sys.path.append(os.path.dirname(__file__))

from ai_models import build_agent

SYSTEM_PROMPT = r"""You are a repair agent for Typst -> LaTeX conversion.

Goal: Fix the deterministic LaTeX output to restore functional parity with the original Typst.
Focus on structure, math, references, figures, and tables. Avoid cosmetic changes.

IMPORTANT OUTPUT RULES:
- Output ONLY the repaired LaTeX source code. No explanations, no markdown fences.
- Do not include analysis or reasoning in the response.
- Do not remove required document structure (documentclass, begin/end document).

GATE AWARENESS:
Your output will be rejected if:
- It introduces LaTeX parse errors
- It increases LaTeX warnings
- It reduces structural metrics (headings/equations/figures/tables/refs/cites/labels)
- It fails to reduce loss markers (unless allowed)

INPUT FORMAT (JSON on stdin):
{
  "input": "<original Typst>",
  "output": "<deterministic LaTeX>",
  "report": { "losses": [...] },
  "metrics": { ... }
}

LOSS MARKERS:
The deterministic output may include LaTeX comments like:
  % tylax:loss:L0001 kind=... message=...
Reduce these by making real fixes (not just deleting content).

REPAIR STRATEGY (do not output these steps):
1) Scan output for loss markers and read matching loss entries.
2) Use the Typst input to infer intent for each loss.
3) Apply minimal edits: preserve structure and content, fix only the gap.
4) Ensure output remains valid LaTeX.

COMMON REPAIR PATTERNS:
- Headings: `= Title` -> \section{Title}, `==` -> \subsection{...}
- Emphasis: *bold* -> \textbf{...}, _italic_ -> \textit{...}, `code` -> \texttt{...}
- References: #ref("x") -> \ref{x}; #label("x") -> \label{x}
- Citations: #cite("x") -> \cite{x}
- Links: #link("url")[text] -> \href{url}{text}
- Math: frac(a,b) -> \frac{a}{b}, sqrt(x) -> \sqrt{x}, bb(R) -> \mathbb{R}

Do NOT delete whole sections, figures, tables, or headings.

FINAL RESPONSE: only the repaired LaTeX source."""


def eprint(msg: str) -> None:
    sys.stderr.write(msg + "\n")


def strip_fences(text: str) -> str:
    fence = re.compile(r"```[a-zA-Z0-9]*\n([\s\S]*?)\n```")
    match = fence.search(text)
    if match:
        return match.group(1).strip()
    return text.strip()


async def run_repair(payload: dict) -> str:
    model_id = os.getenv("TYLAX_AI_MODEL") or os.getenv("OPENROUTER_MODEL")
    reasoning = os.getenv("TYLAX_AI_REASONING", "1") != "0"
    debug_mode = os.getenv("TYLAX_AI_DEBUG", "0") == "1"

    agent = build_agent(
        instructions=SYSTEM_PROMPT,
        model_id=model_id,
        reasoning=reasoning,
        debug_mode=debug_mode,
    )

    prompt = json.dumps(payload, ensure_ascii=False)
    response = await agent.arun(prompt)
    content = getattr(response, "content", response)
    if not isinstance(content, str):
        content = str(content)
    return strip_fences(content)


def main() -> None:
    try:
        payload = json.load(sys.stdin)
    except Exception as exc:
        eprint(f"Failed to read JSON: {exc}")
        sys.exit(1)

    report = payload.get("report") or {}
    losses = report.get("losses") or []

    if not losses:
        print(payload.get("output", ""))
        return

    try:
        repaired = asyncio.run(run_repair(payload))
    except Exception as exc:
        eprint(f"Agno repair failed: {exc}; returning original output")
        print(payload.get("output", ""))
        return

    print(repaired)


if __name__ == "__main__":
    main()
