"""The LLM behind the service. `LLMClient` is the seam; `AnthropicClient` is the default."""

import asyncio
from typing import Protocol

import httpx

RETRY_STATUSES = (429, 529)


class LLMError(Exception):
    """The provider failed or answered something unusable."""


class LLMClient(Protocol):
    async def complete(self, system: str, user: str, schema: dict) -> dict:
        """The answer as an object matching `schema`."""
        ...


class AnthropicClient:
    """The Anthropic Messages API over HTTPS, with no SDK. The answer is forced through a
    tool whose input schema is the result shape, so it is structured rather than parsed."""

    def __init__(
        self,
        api_key: str,
        model: str,
        base_url: str = "https://api.anthropic.com",
        max_tokens: int = 4096,
        timeout: float = 60.0,
        retry_delay: float = 1.0,
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        self._model = model
        self._max_tokens = max_tokens
        self._retry_delay = retry_delay
        self._http = httpx.AsyncClient(
            base_url=base_url,
            timeout=timeout,
            transport=transport,
            headers={
                "x-api-key": api_key,
                "anthropic-version": "2023-06-01",
                "content-type": "application/json",
            },
        )

    async def _post(self, body: dict) -> httpx.Response:
        for attempt in (0, 1):
            try:
                response = await self._http.post("/v1/messages", json=body)
            except httpx.HTTPError as error:
                raise LLMError(f"provider unreachable: {type(error).__name__}") from error
            if response.status_code in RETRY_STATUSES and attempt == 0:
                await asyncio.sleep(self._retry_delay)
                continue
            return response
        raise AssertionError("unreachable")

    async def complete(self, system: str, user: str, schema: dict) -> dict:
        response = await self._post(
            {
                "model": self._model,
                "max_tokens": self._max_tokens,
                "system": system,
                "messages": [{"role": "user", "content": user}],
                "tools": [
                    {
                        "name": "report",
                        "description": "Report the review of the diff.",
                        "input_schema": schema,
                    }
                ],
                "tool_choice": {"type": "tool", "name": "report"},
            }
        )
        if response.status_code != 200:
            raise LLMError(f"provider answered {response.status_code}")
        try:
            data = response.json()
        except ValueError as error:
            raise LLMError("provider answer was not JSON") from error
        if not isinstance(data, dict):
            raise LLMError("provider answer was not an object")
        if data.get("stop_reason") == "max_tokens":
            raise LLMError("the answer was cut off")
        for block in data.get("content") or []:
            if isinstance(block, dict) and block.get("type") == "tool_use":
                result = block.get("input")
                if isinstance(result, dict):
                    return result
        raise LLMError("the provider returned no structured answer")

    async def aclose(self) -> None:
        await self._http.aclose()
