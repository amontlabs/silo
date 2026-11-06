"""The Anthropic client against a mock transport: nothing leaves the machine."""

import asyncio
import json

import httpx
import pytest

from app.lenses import RESULT_SCHEMA
from app.llm import AnthropicClient, LLMError

RESULT = {"summary": "s", "findings": []}


def message(text=None, stop_reason="end_turn"):
    return {
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-5-5",
        "content": [{"type": "text", "text": json.dumps(RESULT) if text is None else text}],
        "stop_reason": stop_reason,
        "stop_sequence": None,
        "usage": {"input_tokens": 1, "output_tokens": 1},
    }


def client(handler):
    return AnthropicClient("key", "claude-sonnet-5-5", transport=httpx.MockTransport(handler))


def run(coroutine):
    return asyncio.run(coroutine)


def test_asks_for_structured_output_and_forces_no_tool():
    seen = []

    def handler(request):
        seen.append(json.loads(request.content))
        return httpx.Response(200, json=message())

    assert run(client(handler).complete("sys", "user", RESULT_SCHEMA)) == RESULT
    body = seen[0]
    assert "tool_choice" not in body and "tools" not in body
    assert body["output_config"] == {"format": {"type": "json_schema", "schema": RESULT_SCHEMA}}
    assert body["max_tokens"] == 4096 and body["model"] == "claude-sonnet-5-5"


def test_the_schema_closes_every_object():
    def objects(node):
        if isinstance(node, dict):
            if node.get("type") == "object":
                yield node
            for value in node.values():
                yield from objects(value)
        elif isinstance(node, list):
            for value in node:
                yield from objects(value)

    found = list(objects(RESULT_SCHEMA))
    assert len(found) == 2 and all(o["additionalProperties"] is False for o in found)


@pytest.mark.parametrize("stop, match", [("max_tokens", "cut off"), ("refusal", "refused")])
def test_a_cut_off_or_refused_answer_is_an_error(stop, match):
    with pytest.raises(LLMError, match=match):
        run(client(lambda r: httpx.Response(200, json=message(stop_reason=stop))).complete("s", "u", RESULT_SCHEMA))


def test_an_answer_that_is_not_json_is_an_error():
    with pytest.raises(LLMError):
        run(client(lambda r: httpx.Response(200, json=message("nope"))).complete("s", "u", RESULT_SCHEMA))


@pytest.mark.parametrize("status", [429, 529])
def test_one_retry_on_overload_honouring_retry_after(status):
    calls = []

    def handler(request):
        calls.append(1)
        if len(calls) == 1:
            return httpx.Response(status, headers={"retry-after-ms": "1"}, json={"type": "error"})
        return httpx.Response(200, json=message())

    assert run(client(handler).complete("s", "u", RESULT_SCHEMA)) == RESULT
    assert len(calls) == 2


def test_no_second_retry_and_no_retry_on_client_errors():
    calls = []

    def always(status):
        def handler(request):
            calls.append(status)
            return httpx.Response(status, headers={"retry-after-ms": "1"}, json={"type": "error"})

        return handler

    with pytest.raises(LLMError, match="429"):
        run(client(always(429)).complete("s", "u", RESULT_SCHEMA))
    assert calls == [429, 429]
    calls.clear()
    with pytest.raises(LLMError, match="400"):
        run(client(always(400)).complete("s", "u", RESULT_SCHEMA))
    assert calls == [400]
