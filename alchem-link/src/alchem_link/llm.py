"""A stdlib LLM client, OpenAI-compatible, with the provider chain this repo already uses.

Same environment variables and base URLs as `scematica-ai` so one set of keys works
across the project. Nothing here is Alchem-Link-specific; the chain-aware part lives in
:mod:`alchem_link.agent`.

Provider priority is **free-and-fast first**, which is the opposite of the Rust side's
priority and deliberate: this is a developer console, not a trading agent, so a free
Groq or OpenRouter key should be picked up before a paid Anthropic one. Set
``ALCHEM_LLM_PROVIDER`` to override.

Only the OpenAI-compatible ``/chat/completions`` shape is implemented — Groq, xAI,
OpenRouter, Cerebras and Ollama all speak it, and it is the one that carries tool
calling. Anthropic's native API has a different message shape and is intentionally not
handled here; point ``ALCHEMY``-style keys at OpenRouter if you want Claude.
"""
from __future__ import annotations

import json
import os
import re
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

DEFAULT_TIMEOUT = 60.0
DEFAULT_MAX_TOKENS = 1024

#: Override the auto-detected provider.
PROVIDER_ENV = "ALCHEM_LLM_PROVIDER"
#: Override the model, whatever the provider.
MODEL_ENV = "ALCHEM_LLM_MODEL"


#: Longest we will block waiting out a rate limit before handing control back.
MAX_BACKOFF_SECS = 20.0

_RETRY_HINT = re.compile(r"try again in ([0-9.]+)\s*(ms|s)\b", re.I)


class LlmError(RuntimeError):
    """The provider could not be reached, or refused the request."""


def _retry_after(exc: Any, detail: str, attempt: int) -> float:
    """How long the provider wants us to wait, from its header or its message."""
    header = ""
    try:
        header = exc.headers.get("retry-after", "") or ""
    except Exception:  # pragma: no cover - header access varies by Python build
        pass
    if header:
        try:
            return float(header)
        except ValueError:
            pass

    # Groq and friends put the precise wait in the error text, not the header.
    match = _RETRY_HINT.search(detail or "")
    if match:
        value = float(match.group(1))
        return value / 1000.0 if match.group(2).lower() == "ms" else value

    return 1.0 * (attempt + 1)


def _rate_limit_text(label: str, code: int, detail: str, wait: float) -> str:
    if code != 429:
        return f"{label} HTTP {code}: {detail}"
    # The raw body is a wall of JSON; the useful part is the wait and the cause.
    reason = "rate limit"
    if "tokens per minute" in detail.lower() or '"tokens"' in detail:
        reason = "token-per-minute limit"
    elif "requests per" in detail.lower():
        reason = "request-per-minute limit"
    return (
        f"{label} hit its {reason}. Retry in about {wait:.0f}s, or set a different "
        f"provider with {PROVIDER_ENV} (see `alchem-link providers`)."
    )


class NoProviderConfigured(LlmError):
    """No API key was found and no local model is reachable."""

    def __init__(self) -> None:
        super().__init__(
            "No LLM provider is configured. Chat needs one of:\n"
            "  GROQ_API_KEY        free tier, fastest — https://console.groq.com\n"
            "  OPENROUTER_API_KEY  free models available — https://openrouter.ai\n"
            "  CEREBRAS_API_KEY    free tier — https://cloud.cerebras.ai\n"
            "  XAI_API_KEY         paid\n"
            "  or a local Ollama on http://localhost:11434\n\n"
            "Every other Alchem-Link command works without one."
        )


@dataclass(frozen=True)
class Provider:
    key: str
    label: str
    base_url: str
    default_model: str
    #: Environment variable holding the API key. Empty for keyless local servers.
    key_env: str = ""
    #: Free tier available without payment details.
    free: bool = False


#: Ordered by preference: free and fast first. Matches `scematica-ai`'s base URLs.
PROVIDERS: Dict[str, Provider] = {
    "groq": Provider(
        key="groq",
        label="Groq",
        base_url="https://api.groq.com/openai/v1",
        default_model="llama-3.3-70b-versatile",
        key_env="GROQ_API_KEY",
        free=True,
    ),
    "cerebras": Provider(
        key="cerebras",
        label="Cerebras",
        base_url="https://api.cerebras.ai/v1",
        default_model="llama-3.3-70b",
        key_env="CEREBRAS_API_KEY",
        free=True,
    ),
    "openrouter": Provider(
        key="openrouter",
        label="OpenRouter",
        base_url="https://openrouter.ai/api/v1",
        # The `:free` suffix is not decoration — it selects the no-cost pool.
        default_model="meta-llama/llama-3.3-70b-instruct:free",
        key_env="OPENROUTER_API_KEY",
        free=True,
    ),
    "xai": Provider(
        key="xai",
        label="xAI",
        base_url="https://api.x.ai/v1",
        default_model="grok-2-latest",
        key_env="XAI_API_KEY",
    ),
    "ollama": Provider(
        key="ollama",
        label="Ollama (local)",
        base_url="http://localhost:11434/v1",
        default_model="llama3.1",
        free=True,
    ),
}

#: Detection order. Keyed providers first — a reachable Ollama should not shadow a key
#: the user deliberately exported.
DETECTION_ORDER = ("groq", "cerebras", "openrouter", "xai", "ollama")


def _ollama_reachable(timeout: float = 0.6) -> bool:
    try:
        request = urllib.request.Request("http://localhost:11434/api/tags")
        with urllib.request.urlopen(request, timeout=timeout):
            return True
    except Exception:
        return False


def detect_provider(env: Optional[Dict[str, str]] = None) -> Optional[Provider]:
    """Pick a provider from the environment, or ``None`` if none is available."""
    environ = os.environ if env is None else env

    forced = environ.get(PROVIDER_ENV, "").strip().lower()
    if forced:
        provider = PROVIDERS.get(forced)
        if provider is None:
            raise LlmError(
                f"unknown provider '{forced}'. Known: {', '.join(sorted(PROVIDERS))}"
            )
        return provider

    for name in DETECTION_ORDER:
        provider = PROVIDERS[name]
        if provider.key_env and environ.get(provider.key_env, "").strip():
            return provider
        if not provider.key_env and _ollama_reachable():
            return provider
    return None


def available_providers(env: Optional[Dict[str, str]] = None) -> List[Dict[str, Any]]:
    """Every provider and whether it is usable right now — for `doctor` and `:providers`."""
    environ = os.environ if env is None else env
    out: List[Dict[str, Any]] = []
    for name in DETECTION_ORDER:
        provider = PROVIDERS[name]
        if provider.key_env:
            ready = bool(environ.get(provider.key_env, "").strip())
            detail = f"set {provider.key_env}" if not ready else f"{provider.key_env} is set"
        else:
            ready = _ollama_reachable()
            detail = "reachable on :11434" if ready else "not running on :11434"
        out.append({
            "provider": provider.key,
            "label": provider.label,
            "model": provider.default_model,
            "free": provider.free,
            "ready": ready,
            "detail": detail,
        })
    return out


@dataclass
class Message:
    role: str
    content: Optional[str] = None
    tool_calls: Optional[List[Dict[str, Any]]] = None
    tool_call_id: Optional[str] = None
    name: Optional[str] = None

    def as_dict(self) -> Dict[str, Any]:
        payload: Dict[str, Any] = {"role": self.role}
        # An assistant turn that is purely tool calls has content None, and some
        # providers reject a missing key while others reject a null one. Empty string
        # is accepted by both.
        payload["content"] = self.content if self.content is not None else ""
        if self.tool_calls:
            payload["tool_calls"] = self.tool_calls
        if self.tool_call_id:
            payload["tool_call_id"] = self.tool_call_id
        if self.name:
            payload["name"] = self.name
        return payload


@dataclass
class Completion:
    content: str
    tool_calls: List[Dict[str, Any]] = field(default_factory=list)
    finish_reason: str = ""
    model: str = ""
    prompt_tokens: int = 0
    completion_tokens: int = 0

    @property
    def wants_tools(self) -> bool:
        return bool(self.tool_calls)


class LlmClient:
    """Minimal OpenAI-compatible chat client over ``urllib``."""

    def __init__(
        self,
        provider: Optional[Provider] = None,
        model: Optional[str] = None,
        timeout: float = DEFAULT_TIMEOUT,
        env: Optional[Dict[str, str]] = None,
        max_tokens: int = DEFAULT_MAX_TOKENS,
    ) -> None:
        environ = os.environ if env is None else env
        resolved = provider or detect_provider(environ)
        if resolved is None:
            raise NoProviderConfigured()
        self.provider = resolved
        self.model = model or environ.get(MODEL_ENV, "").strip() or resolved.default_model
        self.timeout = timeout
        self.max_tokens = max_tokens
        self._api_key = environ.get(resolved.key_env, "").strip() if resolved.key_env else ""
        self.last_error: str = ""

    @property
    def label(self) -> str:
        return f"{self.provider.label} · {self.model}"

    def _headers(self) -> Dict[str, str]:
        headers = {
            "Content-Type": "application/json",
            "User-Agent": "alchem-link",
        }
        if self._api_key:
            headers["Authorization"] = f"Bearer {self._api_key}"
        if self.provider.key == "openrouter":
            # OpenRouter attributes requests to a referring app; without these the
            # request still works but is not credited to anything.
            headers["HTTP-Referer"] = "https://github.com/Meta-Oracle/Scematica"
            headers["X-Title"] = "Alchem-Link"
        return headers

    def chat(
        self,
        messages: List[Message],
        tools: Optional[List[Dict[str, Any]]] = None,
        temperature: float = 0.2,
        retries: int = 1,
    ) -> Completion:
        """One chat completion. Tool definitions are passed through untouched."""
        body: Dict[str, Any] = {
            "model": self.model,
            "messages": [m.as_dict() for m in messages],
            "temperature": temperature,
            "max_tokens": self.max_tokens,
        }
        if tools:
            body["tools"] = tools
            body["tool_choice"] = "auto"

        payload = json.dumps(body).encode("utf-8")
        request = urllib.request.Request(
            f"{self.provider.base_url}/chat/completions",
            data=payload,
            headers=self._headers(),
        )

        last: Optional[Exception] = None
        for attempt in range(max(0, retries) + 1):
            try:
                with urllib.request.urlopen(request, timeout=self.timeout) as response:
                    parsed = json.loads(response.read().decode("utf-8"))
                return self._parse(parsed)
            except urllib.error.HTTPError as exc:
                detail = ""
                try:
                    detail = exc.read().decode("utf-8", "replace")[:400]
                except Exception:  # pragma: no cover - best effort
                    pass
                # 429 and 5xx are worth another try; a 400 means the request itself is
                # wrong and retrying just burns quota.
                if exc.code == 429 or exc.code >= 500:
                    wait = _retry_after(exc, detail, attempt)
                    last = LlmError(_rate_limit_text(self.provider.label, exc.code, detail, wait))
                    # Free tiers meter tokens per minute and say how long to wait. A flat
                    # one-second backoff ignores that and fails a request the provider
                    # was willing to serve seconds later.
                    if attempt < retries and wait <= MAX_BACKOFF_SECS:
                        time.sleep(wait)
                        continue
                    raise last from exc
                raise LlmError(f"{self.provider.label} HTTP {exc.code}: {detail}") from exc
            except (urllib.error.URLError, TimeoutError, OSError) as exc:
                last = LlmError(f"could not reach {self.provider.label}: {exc}")
                if attempt < retries:
                    time.sleep(1.0 * (attempt + 1))
                    continue
                raise last from exc
            except json.JSONDecodeError as exc:
                raise LlmError(f"{self.provider.label} returned a non-JSON body") from exc
        raise last or LlmError("chat failed")

    def _parse(self, body: Dict[str, Any]) -> Completion:
        if "error" in body and body["error"]:
            error = body["error"]
            message = error.get("message", str(error)) if isinstance(error, dict) else str(error)
            raise LlmError(f"{self.provider.label}: {message}")

        choices = body.get("choices") or []
        if not choices:
            raise LlmError(f"{self.provider.label} returned no choices")

        choice = choices[0]
        message = choice.get("message") or {}
        usage = body.get("usage") or {}
        return Completion(
            content=(message.get("content") or "").strip(),
            tool_calls=list(message.get("tool_calls") or []),
            finish_reason=choice.get("finish_reason", ""),
            model=body.get("model", self.model),
            prompt_tokens=int(usage.get("prompt_tokens") or 0),
            completion_tokens=int(usage.get("completion_tokens") or 0),
        )
