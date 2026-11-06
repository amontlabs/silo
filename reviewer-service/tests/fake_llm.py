"""A fake LLM client for tests: answers per lens from a table and records what it was asked."""

import json
import re


class FakeLLM:
    def __init__(self, answers: dict | None = None, fail: Exception | None = None) -> None:
        self.answers = answers or {}
        self.fail = fail
        self.calls: list[tuple[str, str]] = []

    async def complete(self, system: str, user: str) -> str:
        self.calls.append((system, user))
        if self.fail:
            raise self.fail
        lens = re.match(r"Lens: (\w+)\.", user).group(1)
        default = {
            "summary": f"{lens} read of the diff.",
            "findings": [{"severity": "low", "file": "~/app/run.sh", "message": f"{lens} note."}],
        }
        return "Here you go:\n" + json.dumps(self.answers.get(lens, default))
