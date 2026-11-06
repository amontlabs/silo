"""Runs the service on a local port with a fake LLM, for the CLI's end-to-end test.

Usage: python serve_fake.py PORT TOKEN [ANSWERS_JSON]. Prints `ready` once it listens.
"""

import json
import os
import sys

import uvicorn

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
sys.path.insert(0, os.path.dirname(__file__))

from app.main import create_app  # noqa: E402
from fake_llm import FakeLLM  # noqa: E402

if __name__ == "__main__":
    port, token = int(sys.argv[1]), sys.argv[2]
    answers = json.loads(sys.argv[3]) if len(sys.argv) > 3 else None
    app = create_app(FakeLLM(answers), [token])
    print("ready", flush=True)
    uvicorn.run(app, host="127.0.0.1", port=port, log_level="warning")
