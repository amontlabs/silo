"""A minimal, standard-library client for Silo's engine socket.

The engine (`silod`) speaks length-prefixed JSON on a Unix socket: each frame is a
4-byte big-endian length followed by a UTF-8 JSON object. The first request on a
connection must be `engine.handshake`. Requests and responses are matched by `id`.
See `app/SiloUI/client/src/codec.rs` and `app/SiloUI/api/src/transport.rs`.

This client is deliberately small so that the evals do not depend on building the
Rust workspace. It runs on the stock macOS `python3` (3.9).
"""
from __future__ import annotations

import json
import os
import socket
import struct
import time
import uuid
from pathlib import Path
from typing import Any, Dict, List, Optional

PROTOCOL_VERSION = 8
MAX_FRAME_BYTES = 4 * 1024 * 1024
CHANNEL_DIRECTORIES = {"production": ".silo", "development": ".silo-dev"}


class EngineError(Exception):
    """The engine answered a request with a typed error."""

    def __init__(self, method: str, code: str, message: str):
        super().__init__(f"{method}: {code}: {message}")
        self.method = method
        self.code = code
        self.message = message


def socket_path(channel: str = "production", home: Optional[str] = None) -> Path:
    base = Path(home or os.path.expanduser("~"))
    return base / CHANNEL_DIRECTORIES[channel] / "engine.sock"


def encode_frame(value: Any) -> bytes:
    payload = json.dumps(value, separators=(",", ":")).encode("utf-8")
    if not payload or len(payload) > MAX_FRAME_BYTES:
        raise ValueError(f"frame length {len(payload)} is outside the allowed range")
    return struct.pack(">I", len(payload)) + payload


def _read_exact(sock: socket.socket, count: int) -> bytes:
    chunks = []
    remaining = count
    while remaining:
        chunk = sock.recv(remaining)
        if not chunk:
            raise ConnectionError("the engine closed the connection")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def read_frame(sock: socket.socket) -> Dict[str, Any]:
    (length,) = struct.unpack(">I", _read_exact(sock, 4))
    if length == 0 or length > MAX_FRAME_BYTES:
        raise ValueError(f"frame length {length} is outside the allowed range")
    return json.loads(_read_exact(sock, length).decode("utf-8"))


class EngineClient:
    """One connection to `engine.sock`. Not thread-safe; the evals are sequential."""

    def __init__(self, path: Path, timeout: float = 240.0, client_name: str = "silo-isolation-evals"):
        self.path = Path(path)
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(timeout)
        self.sock.connect(str(self.path))
        self.methods: List[str] = []
        self.handshake = self._call(
            "engine.handshake",
            {
                "protocol": {"min": PROTOCOL_VERSION, "max": PROTOCOL_VERSION},
                "client": {"name": client_name, "version": "1"},
            },
            request_id="handshake",
        )

    def close(self) -> None:
        try:
            self.sock.close()
        except OSError:
            pass

    def __enter__(self) -> "EngineClient":
        return self

    def __exit__(self, *_exc: Any) -> None:
        self.close()

    def _call(self, method: str, params: Dict[str, Any], request_id: Optional[str] = None) -> Any:
        request_id = request_id or uuid.uuid4().hex
        if method not in self.methods:
            self.methods.append(method)
        self.sock.sendall(encode_frame({"id": request_id, "method": method, "params": params}))
        while True:
            frame = read_frame(self.sock)
            if frame.get("id") != request_id:
                # Events and unrelated frames are not used by the evals.
                continue
            if "error" in frame:
                error = frame["error"] or {}
                raise EngineError(method, str(error.get("code")), str(error.get("message")))
            return frame.get("result")

    def call(self, method: str, params: Optional[Dict[str, Any]] = None) -> Any:
        return self._call(method, params or {})

    # Jobs ---------------------------------------------------------------------------

    def wait_job(self, job_id: str, timeout: float = 600.0, poll: float = 1.0) -> Any:
        deadline = time.monotonic() + timeout
        while True:
            found = self.call("operations.job.get", {"jobId": job_id})
            if found.get("state") == "finished":
                outcome = found.get("outcome") or {}
                if outcome.get("error"):
                    error = outcome["error"]
                    raise EngineError(job_id, str(error.get("code")), str(error.get("message")))
                return outcome.get("result")
            if time.monotonic() > deadline:
                raise TimeoutError(f"job {job_id} did not finish within {timeout:.0f}s")
            time.sleep(poll)

    # Computers ----------------------------------------------------------------------

    def computers(self) -> List[Dict[str, Any]]:
        listed = self.call("computers.list", {})
        return list((listed.get("linux") or {}).get("computers") or [])

    def computer_by_name(self, name: str) -> Dict[str, Any]:
        for computer in self.computers():
            if computer["configuration"]["name"] == name:
                return computer
        raise LookupError(f"no Linux computer named {name!r}")

    def restart(self, computer_id: str) -> None:
        started = self.call("computers.restart", {"computerId": computer_id})
        self.wait_job(started["jobId"])

    # Guest processes ----------------------------------------------------------------

    def exec(
        self,
        computer_id: str,
        argv: List[str],
        *,
        stdin: Optional[bytes] = None,
        root: bool = True,
        timeout_ms: int = 60_000,
        env: Optional[Dict[str, str]] = None,
        max_output_bytes: int = 768 * 1024,
    ) -> "ExecResult":
        params: Dict[str, Any] = {
            "computerId": computer_id,
            "argv": argv,
            "as": "root" if root else "account",
            "timeoutMs": timeout_ms,
            "maxOutputBytes": max_output_bytes,
        }
        if stdin is not None:
            params["stdin"] = list(stdin)
        if env:
            params["env"] = [{"name": k, "value": v} for k, v in env.items()]
        result = self.call("guest.exec", params)
        return ExecResult(
            exit_code=result.get("exitCode"),
            stdout=bytes(result.get("stdout") or []),
            stderr=bytes(result.get("stderr") or []),
        )

    def exec_start(self, computer_id: str, argv: List[str], *, stdin: Optional[bytes] = None, root: bool = True) -> str:
        params: Dict[str, Any] = {
            "computerId": computer_id,
            "argv": argv,
            "as": "root" if root else "account",
            "stdinOpen": False,
            "retainOutputBytes": 768 * 1024,
        }
        if stdin is not None:
            params["stdin"] = list(stdin)
        return self.call("guest.exec.start", params)["jobId"]

    def exec_get(self, job_id: str) -> Dict[str, Any]:
        return self.call("guest.exec.get", {"jobId": job_id})

    def exec_wait(self, job_id: str, timeout: float) -> "ExecResult":
        deadline = time.monotonic() + timeout
        while True:
            state = self.exec_get(job_id)
            if state["state"] != "running":
                stdout = b"".join(bytes(o["data"]) for o in state["output"] if o["stream"] == "stdout")
                stderr = b"".join(bytes(o["data"]) for o in state["output"] if o["stream"] == "stderr")
                return ExecResult(exit_code=state.get("exitCode"), stdout=stdout, stderr=stderr)
            if time.monotonic() > deadline:
                self.cancel(job_id)
                raise TimeoutError(f"guest job {job_id} did not finish within {timeout:.0f}s")
            time.sleep(2)

    def cancel(self, job_id: str) -> None:
        try:
            self.call("operations.cancel", {"jobId": job_id})
        except EngineError:
            pass


class ExecResult:
    def __init__(self, exit_code: Optional[int], stdout: bytes, stderr: bytes):
        self.exit_code = exit_code
        self.stdout = stdout
        self.stderr = stderr

    def json(self) -> Any:
        return json.loads(self.stdout.decode("utf-8"))

    def text(self) -> str:
        return self.stdout.decode("utf-8", "replace")

    def __repr__(self) -> str:
        return f"ExecResult(exit={self.exit_code}, stdout={self.stdout[:200]!r}, stderr={self.stderr[:200]!r})"
