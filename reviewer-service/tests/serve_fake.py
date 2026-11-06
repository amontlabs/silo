"""Runs the service on a free local port with a fake LLM, for the CLI's end-to-end test.

Usage: python serve_fake.py TOKEN [ANSWERS_JSON]. Prints `ready PORT` once it listens. The
socket is bound here, so the port cannot be taken between choosing it and listening.
"""

import json
import os
import socket
import sys

import uvicorn

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
sys.path.insert(0, os.path.dirname(__file__))

from app.main import create_app  # noqa: E402
from fake_llm import FakeLLM  # noqa: E402

if __name__ == "__main__":
    token = sys.argv[1]
    answers = json.loads(sys.argv[2]) if len(sys.argv) > 2 else None
    sock = socket.socket()
    sock.bind(("127.0.0.1", 0))
    sock.listen(16)
    print("ready", sock.getsockname()[1], flush=True)
    app = create_app(FakeLLM(answers), [token])
    uvicorn.Server(uvicorn.Config(app, log_level="warning")).run(sockets=[sock])
