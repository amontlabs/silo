"""The service over real HTTP on localhost, with a fake LLM client. Nothing leaves the machine."""

import json
import logging
import socket
import threading
import time

import httpx
import pytest
import uvicorn

from app.lenses import FILTERED_WORDS, SYSTEM_PROMPT
from app.llm import LLMError
from app.main import create_app
from fake_llm import FakeLLM

TOKEN = "test-token"
DIFF = {"summary": [], "code": [], "packages": [{"manager": "apt", "added": {"jq": "1.7"}}]}


class Server:
    def __init__(self, client, max_bytes=4096):
        sock = socket.socket()
        sock.bind(("127.0.0.1", 0))
        self.port = sock.getsockname()[1]
        sock.close()
        config = uvicorn.Config(
            create_app(client, [TOKEN, "second-token"], max_bytes),
            host="127.0.0.1",
            port=self.port,
            log_level="warning",
        )
        self.server = uvicorn.Server(config)
        self.thread = threading.Thread(target=self.server.run, daemon=True)

    def __enter__(self):
        self.thread.start()
        for _ in range(100):
            if self.server.started:
                return self
            time.sleep(0.05)
        raise RuntimeError("server did not start")

    def __exit__(self, *_):
        self.server.should_exit = True
        self.thread.join(5)

    def post(self, body, token=TOKEN, **kwargs):
        headers = {"Authorization": f"Bearer {token}"} if token else {}
        return httpx.post(
            f"http://127.0.0.1:{self.port}/v1/review", json=body, headers=headers, **kwargs
        )


def test_reviews_each_lens():
    llm = FakeLLM()
    with Server(llm) as server:
        response = server.post({"diff": DIFF, "lenses": ["general", "code"]})
    assert response.status_code == 200
    lenses = response.json()["lenses"]
    assert set(lenses) == {"general", "code"}
    assert lenses["code"]["findings"][0] == {
        "severity": "low",
        "file": "~/app/run.sh",
        "message": "code note.",
    }
    assert len(llm.calls) == 2
    assert all('"jq"' in user for _, user in llm.calls)


def test_all_lenses_by_default_and_answers_are_normalised():
    answers = {
        "general": {
            "summary": "s",
            "findings": [{"severity": "CRITICAL", "message": "m"}, "junk", {"severity": "High"}],
        }
    }
    with Server(FakeLLM(answers)) as server:
        response = server.post({"diff": DIFF})
    lenses = response.json()["lenses"]
    assert list(lenses) == ["general", "packages", "code"]
    assert [f["severity"] for f in lenses["general"]["findings"]] == ["info", "high"]


def test_prompt_names_the_filtered_words_and_forbids_verdicts():
    for word in FILTERED_WORDS:
        assert word in SYSTEM_PROMPT
    assert "approved" in SYSTEM_PROMPT and "Do not give a verdict" in SYSTEM_PROMPT


def test_a_wrong_or_missing_token_is_refused_before_any_work():
    llm = FakeLLM()
    with Server(llm) as server:
        assert server.post({"diff": DIFF}, token="nope").status_code == 401
        assert server.post({"diff": DIFF}, token=None).status_code == 401
        assert server.post({"diff": DIFF}, token="second-token").status_code == 200
    assert len(llm.calls) == 3


def test_a_large_body_is_refused():
    llm = FakeLLM()
    with Server(llm, max_bytes=1000) as server:
        response = server.post({"diff": {"x": "y" * 5000}})
    assert response.status_code == 413
    assert llm.calls == []


@pytest.mark.parametrize(
    "body",
    [{"lenses": ["general"]}, {"diff": []}, {"diff": {}, "lenses": ["style"]}, {"diff": {}, "lenses": []}],
)
def test_a_malformed_request_is_a_400(body):
    with Server(FakeLLM()) as server:
        assert server.post(body).status_code == 400


def test_a_provider_failure_is_a_502_without_detail():
    with Server(FakeLLM(fail=LLMError("provider answered 500"))) as server:
        response = server.post({"diff": DIFF, "lenses": ["code"]})
    assert response.status_code == 502
    assert "500" not in response.text


def test_an_answer_that_is_not_json_is_a_502():
    with Server(FakeLLM({"code": None})) as server:
        # `null` parses as JSON but not as an object.
        response = server.post({"diff": DIFF, "lenses": ["code"]})
    assert response.status_code == 502


def test_logs_are_structured_and_hold_no_diff(caplog):
    secret = "needle-in-the-diff"
    with caplog.at_level(logging.INFO, logger="reviewer"):
        with Server(FakeLLM()) as server:
            server.post({"diff": {"other": [{"path": secret}]}, "lenses": ["general"]})
            server.post({"diff": {"x": secret}}, token="wrong")
    assert caplog.records
    for record in caplog.records:
        fields = json.loads(record.getMessage())
        assert "status" in fields
        assert secret not in record.getMessage()
