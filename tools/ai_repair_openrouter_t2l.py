#!/usr/bin/env python3
import json
import os
import re
import sys
import urllib.request

API_URL = os.getenv("OPENROUTER_API_URL", "https://openrouter.ai/api/v1/chat/completions")
DEFAULT_MODEL = "x-ai/grok-4.1-fast"

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


def eprint(msg):
    sys.stderr.write(msg + "\n")


def strip_fences(text):
    fence = re.compile(r"```[a-zA-Z0-9]*\n([\s\S]*?)\n```")
    m = fence.search(text)
    if m:
        return m.group(1).strip()
    return text.strip()


def main():
    try:
        payload = json.load(sys.stdin)
    except Exception as exc:
        eprint(f"Failed to read JSON: {exc}")
        sys.exit(1)

    output = payload.get("output", "")
    report = payload.get("report", {})
    losses = report.get("losses", [])
    if not losses:
        print(output)
        return

    api_key = os.getenv("OPENROUTER_API_KEY")
    if not api_key:
        eprint("OPENROUTER_API_KEY is not set; returning original output")
        print(output)
        return

    model = os.getenv("OPENROUTER_MODEL", DEFAULT_MODEL)
    max_tokens = int(os.getenv("OPENROUTER_MAX_TOKENS", "32000"))
    temperature = float(os.getenv("OPENROUTER_TEMPERATURE", "0"))

    user_content = json.dumps(payload, ensure_ascii=False)

    req_body = {
        "model": model,
        "temperature": temperature,
        "max_tokens": max_tokens,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": user_content},
        ],
    }

    data = json.dumps(req_body).encode("utf-8")
    req = urllib.request.Request(API_URL, data=data, method="POST")
    req.add_header("Authorization", f"Bearer {api_key}")
    req.add_header("Content-Type", "application/json")

    site = os.getenv("OPENROUTER_SITE")
    title = os.getenv("OPENROUTER_APP_NAME")
    if site:
        req.add_header("HTTP-Referer", site)
    if title:
        req.add_header("X-Title", title)

    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            raw = resp.read()
    except Exception as exc:
        eprint(f"OpenRouter request failed: {exc}; returning original output")
        print(output)
        return

    try:
        resp_obj = json.loads(raw)
        content = resp_obj["choices"][0]["message"]["content"]
    except Exception as exc:
        eprint(f"Failed to parse OpenRouter response: {exc}; returning original output")
        print(output)
        return

    if not content:
        print(output)
        return

    print(strip_fences(content))


if __name__ == "__main__":
    main()
