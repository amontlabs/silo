"""The LLM behind the service. `LLMClient` is the seam; `AnthropicClient` is the default."""

import json
from typing import Protocol

import anthropic
import httpx


class LLMError(Exception):
    """The provider failed, refused, or answered something unusable."""


class LLMClient(Protocol):
    async def complete(self, system: str, user: str, schema: dict) -> dict:
        """The answer as an object matching `schema`."""
        ...


class AnthropicClient:
    """The Anthropic Messages API through the official SDK. The answer is constrained by a
    JSON schema (structured outputs); no tool choice is forced. The SDK retries once on
    connection errors, 429 and 5xx (including 529) and waits as long as retry-after asks."""

    def __init__(
        self,
        api_key: str,
        model: str,
        base_url: str = "https://api.anthropic.com",
        max_tokens: int = 4096,
        timeout: float = 60.0,
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        self._model = model
        self._max_tokens = max_tokens
        self._sdk = anthropic.AsyncAnthropic(
            api_key=api_key,
            base_url=base_url,
            timeout=timeout,
            max_retries=1,
            http_client=httpx.AsyncClient(transport=transport, timeout=timeout)
            if transport
            else None,
        )

    async def complete(self, system: str, user: str, schema: dict) -> dict:
        try:
            message = await self._sdk.messages.create(
                model=self._model,
                max_tokens=self._max_tokens,
                system=system,
                messages=[{"role": "user", "content": user}],
                output_config={"format": {"type": "json_schema", "schema": schema}},
            )
        except anthropic.APIStatusError as error:
            raise LLMError(f"provider answered {error.status_code}") from error
        except anthropic.APIError as error:
            raise LLMError(f"provider unreachable: {type(error).__name__}") from error
        if message.stop_reason == "refusal":
            raise LLMError("the provider refused")
        if message.stop_reason == "max_tokens":
            raise LLMError("the answer was cut off")
        text = "".join(b.text for b in message.content if b.type == "text")
        try:
            result = json.loads(text)
        except ValueError as error:
            raise LLMError("the answer was not JSON") from error
        if not isinstance(result, dict):
            raise LLMError("the answer was not an object")
        return result

    async def aclose(self) -> None:
        await self._sdk.close()
