"""The LLM behind the service. `LLMClient` is the seam; `AnthropicClient` is the default."""

from typing import Protocol

import httpx


class LLMError(Exception):
    """The provider failed or answered something unusable."""


class LLMClient(Protocol):
    async def complete(self, system: str, user: str) -> str: ...


class AnthropicClient:
    """The Anthropic Messages API over HTTPS, with no SDK."""

    def __init__(
        self,
        api_key: str,
        model: str,
        base_url: str = "https://api.anthropic.com",
        max_tokens: int = 2000,
        timeout: float = 60.0,
    ) -> None:
        self._model = model
        self._max_tokens = max_tokens
        self._http = httpx.AsyncClient(
            base_url=base_url,
            timeout=timeout,
            headers={
                "x-api-key": api_key,
                "anthropic-version": "2023-06-01",
                "content-type": "application/json",
            },
        )

    async def complete(self, system: str, user: str) -> str:
        try:
            response = await self._http.post(
                "/v1/messages",
                json={
                    "model": self._model,
                    "max_tokens": self._max_tokens,
                    "system": system,
                    "messages": [{"role": "user", "content": user}],
                },
            )
        except httpx.HTTPError as error:
            raise LLMError(f"provider unreachable: {type(error).__name__}") from error
        if response.status_code != 200:
            raise LLMError(f"provider answered {response.status_code}")
        blocks = response.json().get("content") or []
        return "".join(b.get("text", "") for b in blocks if b.get("type") == "text")

    async def aclose(self) -> None:
        await self._http.aclose()
