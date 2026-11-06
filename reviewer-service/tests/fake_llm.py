"""A fake LLM client for tests: answers per lens from a table and records what it was asked."""

import re


class FakeLLM:
    def __init__(self, answers: dict | None = None, fail: dict | Exception | None = None) -> None:
        self.answers = answers or {}
        self.fail = fail
        self.calls: list[tuple[str, str]] = []
        self.closed = False

    async def complete(self, system: str, user: str, schema: dict) -> dict:
        self.calls.append((system, user))
        lens = re.search(r"Lens: (\w+)\.", user).group(1)
        failure = self.fail.get(lens) if isinstance(self.fail, dict) else self.fail
        if failure:
            raise failure
        default = {
            "summary": f"{lens} read of the diff.",
            "findings": [{"severity": "low", "file": "~/app/run.sh", "message": f"{lens} note."}],
        }
        return self.answers.get(lens, default)

    async def aclose(self) -> None:
        self.closed = True
