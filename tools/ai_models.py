import os
from typing import Optional

try:
    from agno.agent import Agent
    from agno.models.openrouter import OpenRouter
except ImportError as exc:  # pragma: no cover
    raise RuntimeError(
        "agno is not installed. Install with `pip install agno`."
    ) from exc

OR_GROK_4_1_FAST = "x-ai/grok-4.1-fast"

OPENROUTER_HEADERS = {
    "HTTP-Referer": os.getenv("OPENROUTER_SITE", "https://www.typetex.app"),
    "X-Title": os.getenv("OPENROUTER_APP_NAME", "TypeTeX"),
}

DEFAULT_MAX_TOKENS = int(os.getenv("OPENROUTER_MAX_TOKENS", "32000"))
DEFAULT_TEMPERATURE = float(os.getenv("OPENROUTER_TEMPERATURE", "0"))


def _get_api_key() -> str:
    return os.getenv("OPEN_ROUTER_API_KEY") or os.getenv("OPENROUTER_API_KEY") or ""


def build_openrouter_model(
    model_id: Optional[str] = None,
    reasoning: bool = True,
    max_tokens: Optional[int] = None,
    temperature: Optional[float] = None,
    reasoning_effort: Optional[str] = None,
) -> OpenRouter:
    api_key = _get_api_key()
    if not api_key:
        raise RuntimeError("OPEN_ROUTER_API_KEY or OPENROUTER_API_KEY is not set")

    request_params = {}
    if reasoning:
        request_params = {"extra_body": {"reasoning": {"enabled": True}}}

    kwargs = {}
    if reasoning_effort:
        kwargs["reasoning_effort"] = reasoning_effort

    return OpenRouter(
        id=model_id or OR_GROK_4_1_FAST,
        api_key=api_key,
        max_tokens=max_tokens or DEFAULT_MAX_TOKENS,
        temperature=temperature if temperature is not None else DEFAULT_TEMPERATURE,
        request_params=request_params or None,
        default_headers=OPENROUTER_HEADERS,
        **kwargs,
    )


def build_agent(
    instructions: str,
    model_id: Optional[str] = None,
    reasoning: bool = True,
    retries: int = 2,
    delay_between_retries: int = 1,
    exponential_backoff: bool = True,
    debug_mode: bool = False,
) -> Agent:
    model = build_openrouter_model(model_id=model_id, reasoning=reasoning)
    return Agent(
        model=model,
        name="Tylax Repair Agent",
        markdown=False,
        debug_mode=debug_mode,
        instructions=instructions,
        retries=retries,
        delay_between_retries=delay_between_retries,
        exponential_backoff=exponential_backoff,
    )
