#!/usr/bin/env python3
import json
import os
import re
import sys
import urllib.request

API_URL = os.getenv("OPENROUTER_API_URL", "https://openrouter.ai/api/v1/chat/completions")
DEFAULT_MODEL = "x-ai/grok-4.1-fast"

SYSTEM_PROMPT = r"""You are a repair agent for LaTeX -> Typst conversion.

Goal: Fix the deterministic Typst output to restore functional parity with the original LaTeX.
Focus on structure, math, references, figures, and tables. Avoid cosmetic changes.

IMPORTANT OUTPUT RULES:
- Output ONLY the repaired Typst source code. No explanations, no markdown fences.
- Do not include analysis or reasoning in the response.
- Do not introduce Typst subset violations:
  - No code blocks `{ ... }`
  - No show rules
  - No place(...)
  - No calc.*
  - No spread `..` or `...`
  - No functional collection methods (map/filter/fold/reduce/join)

GATE AWARENESS:
Your output will be rejected if:
- It introduces Typst parse errors
- It increases subset lint issues
- It reduces structural metrics (headings/equations/figures/tables/refs/cites/labels)
- It fails to reduce loss markers (unless allowed)

INPUT FORMAT (JSON on stdin):
{
  "input": "<original LaTeX>",
  "output": "<deterministic Typst>",
  "report": { "losses": [...] },
  "metrics": { ... }
}

LOSS MARKERS:
The deterministic output may include loss markers like:
  /* tylax:loss:L0001 */
These must be reduced by making a real fix (not just deleting content).

REPAIR STRATEGY (do not output these steps):
1) Scan output for loss markers and read matching loss entries.
2) Use the LaTeX input to infer intent for each loss.
3) Apply minimal edits: preserve structure and content, fix only the gap.
4) Ensure the output remains valid Typst within the subset.

COMMON REPAIR PATTERNS:
- Unknown wrapper macros in text: remove the command, keep inner content.
  Example: \myemph{X} -> X (or *X* if emphasis is likely).
- Unknown wrapper macros in math: remove command, keep argument inside.
- Reference-like macros:
  - \ref{X}, \autoref{X} -> #ref("X")
  - \eqref{X} -> #ref("X") (math context)
  - \label{X} -> #label("X")
  - \cite{X}, \citep{X}, \citet{X} -> #cite("X")
- URLs/links:
  - \url{X} -> #link("X")
  - \href{U}{T} -> #link("U")[T]
- Text formatting:
  - \textbf{X} -> *X*
  - \textit{X} / \emph{X} -> _X_
  - \texttt{X} -> `X`
- Common math styling (best-effort):
  - \mathbb{R} -> bb(R)
  - \mathbf{x} -> bold(x)
  - \mathrm{x} -> upright(x)

Do NOT delete whole sections, figures, tables, or headings.

FINAL RESPONSE: only the repaired Typst source."""


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
