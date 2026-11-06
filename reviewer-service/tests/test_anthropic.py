"""The Anthropic client against a mock transport: nothing leaves the machine."""

import asyncio
import json

import httpx
import pytest

from app.lenses import RESULT_SCHEMA
from app.llm import AnthropicClient, LLMError

OK = {
    "stop_reason": "tool_use",
    "content": [
        {"type": "text", "text": "thinking"},
        {"type": "tool_use", "name": "report", "input": {"summary": "s", "findings": []}},
    ],
}


def client(handler):
    return AnthropicClient(
        "key", "claude-sonnet-5-5", retry_delay=0, transport=httpx.MockTransport(handler)
    )


def run(coroutine):
    return asyncio.run(coroutine)


def test_forces_the_report_tool_and_reads_its_input():
    seen = []

    def handler(request):
        seen.append(json.loads(request.content))
        return httpx.Response(200, json=OK)

    answer = run(client(handler).complete("sys", "user", RESULT_SCHEMA))
    assert answer == {"summary": "s", "findings": []}
    body = seen[0]
    assert body["tool_choice"] == {"type": "tool", "name": "report"}
    assert body["tools"][0]["input_schema"] == RESULT_SCHEMA
    assert body["max_tokens"] == 4096 and body["model"] == "claude-sonnet-5-5"


def test_a_cut_off_answer_is_an_error():
    cut = {**OK, "stop_reason": "max_tokens"}
    with pytest.raises(LLMError, match="cut off"):
        run(client(lambda r: httpx.Response(200, json=cut)).complete("s", "u", RESULT_SCHEMA))


def test_an_answer_without_the_tool_is_an_error():
    plain = {"stop_reason": "end_turn", "content": [{"type": "text", "text": "{}"}]}
    with pytest.raises(LLMError):
        run(client(lambda r: httpx.Response(200, json=plain)).complete("s", "u", RESULT_SCHEMA))


@pytest.mark.parametrize("status", [429, 529])
def test_one_retry_on_overload(status):
    calls = []

    def handler(request):
        calls.append(1)
        return httpx.Response(status if len(calls) == 1 else 200, json=OK)

    assert run(client(handler).complete("s", "u", RESULT_SCHEMA))["summary"] == "s"
    assert len(calls) == 2


def test_no_second_retry_and_no_retry_on_other_errors():
    calls = []

    def always(status):
        def handler(request):
            calls.append(status)
            return httpx.Response(status, json={})

        return handler

    with pytest.raises(LLMError, match="429"):
        run(client(always(429)).complete("s", "u", RESULT_SCHEMA))
    assert calls == [429, 429]
    calls.clear()
    with pytest.raises(LLMError, match="500"):
        run(client(always(500)).complete("s", "u", RESULT_SCHEMA))
    assert calls == [500]
