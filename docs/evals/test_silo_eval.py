"""Self-tests for the isolation evals, runnable on any POSIX host without Silo.

A fake engine speaks the real wire format on a Unix socket. Its "guest" runs every
command directly on the test machine, i.e. a computer with no isolation at all. The
evals must report that as a failure: these tests prove the scenarios can detect a
broken boundary, not only that they print "pass".

Run: python3 -m unittest discover -s docs/evals -p 'test_*.py'
"""
from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from typing import Any, Dict
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import engine_client  # noqa: E402
import silo_eval  # noqa: E402


class FakeEngine:
    """Serves the subset of methods the evals call. Guest commands run on this machine."""

    def __init__(self, path: Path, guest_env: Dict[str, str]):
        self.path = path
        self.guest_env = guest_env
        self.secrets: list = []
        self.jobs: Dict[str, subprocess.Popen] = {}
        self.calls: list = []
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server.bind(str(path))
        self.server.listen(4)
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()

    def computer(self, name: str, id: str) -> Dict[str, Any]:
        return {"kind": "linux", "configuration": {"id": id, "name": name}, "state": "running", "githubRepositories": []}

    def serve(self) -> None:
        while True:
            try:
                connection, _ = self.server.accept()
            except OSError:
                return
            threading.Thread(target=self.session, args=(connection,), daemon=True).start()

    def session(self, connection: socket.socket) -> None:
        with connection:
            while True:
                try:
                    request = engine_client.read_frame(connection)
                except (ConnectionError, OSError):
                    return
                self.calls.append(request["method"])
                try:
                    reply = {"id": request["id"], "result": self.handle(request["method"], request["params"])}
                except LookupError as error:
                    reply = {"id": request["id"], "error": {"code": "unavailable", "message": str(error)}}
                connection.sendall(engine_client.encode_frame(reply))

    def environment(self, extra) -> Dict[str, str]:
        env = dict(os.environ)
        env.update(self.guest_env)
        for pair in extra or []:
            env[pair["name"]] = pair["value"]
        return env

    def handle(self, method: str, params: Dict[str, Any]) -> Any:
        if method == "engine.handshake":
            return {"deviceId": "fake", "name": "fake", "protocol": 6, "capabilities": [], "clientId": "client:1"}
        if method == "computers.list":
            return {"linux": {"computers": [self.computer("silo-eval-a", "id-a"), self.computer("silo-eval-b", "id-b")]}, "macos": {}}
        if method == "guest.exec":
            stdin = bytes(params.get("stdin") or [])
            run = subprocess.run(params["argv"], input=stdin, capture_output=True, env=self.environment(params.get("env")), timeout=params["timeoutMs"] / 1000)
            return {"exitCode": run.returncode, "stdout": list(run.stdout), "stderr": list(run.stderr)}
        if method == "guest.exec.start":
            process = subprocess.Popen(params["argv"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=self.environment(params.get("env")))
            process.stdin.write(bytes(params.get("stdin") or []))
            process.stdin.close()
            job = f"guest:{len(self.jobs)}"
            self.jobs[job] = process
            return {"jobId": job}
        if method == "guest.exec.get":
            process = self.jobs[params["jobId"]]
            if process.poll() is None:
                return {"state": "running", "output": [], "firstRetainedSeq": 0}
            out, err = process.stdout.read(), process.stderr.read()
            return {"state": "exited", "exitCode": process.returncode, "firstRetainedSeq": 0,
                    "output": [{"seq": 0, "stream": "stdout", "data": list(out)}, {"seq": 1, "stream": "stderr", "data": list(err)}]}
        if method == "operations.cancel":
            process = self.jobs.get(params["jobId"])
            if process and process.poll() is None:
                process.kill()
            return None
        if method == "secrets.list":
            return self.secrets
        if method == "secrets.save":
            request = params["request"]
            self.secrets = [{"id": "secret:1", "name": request["name"], "computerIds": request["computerIds"], "allowedDomains": request["allowedDomains"]}]
            self.saved_value = request["value"]
            return self.secrets
        if method == "secrets.remove":
            self.secrets = []
            return []
        if method == "computers.restart":
            return {"jobId": "computers:1"}
        if method == "operations.job.get":
            return {"jobId": params["jobId"], "state": "finished", "outcome": {"result": None}}
        if method == "network.publish":
            raise LookupError("publishing is not part of the fake engine")
        raise LookupError(f"unsupported method {method}")

    def close(self) -> None:
        for process in self.jobs.values():
            if process.poll() is None:
                process.kill()
        self.server.close()


class FakeEngineCase(unittest.TestCase):
    guest_env: Dict[str, str] = {}

    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.home = Path(temporary.name)
        (self.home / ".silo").mkdir(mode=0o700)
        self.engine = FakeEngine(self.home / ".silo" / "engine.sock", dict(self.guest_env))
        self.addCleanup(self.engine.close)
        patcher = mock.patch.dict(os.environ, {"HOME": str(self.home)})
        patcher.start()
        self.addCleanup(patcher.stop)

    def run_evals(self, *extra: str) -> Dict[str, Dict[str, Any]]:
        results = self.home / "results"
        args = ["--results", str(results), "--scan-root", str(self.home), "--scan-min-files", "1", "--app", str(self.home / "none.app"), *extra]
        silo_eval.main(args)
        reports = sorted(results.glob("*.json"))
        self.assertEqual(len(reports), 1)
        self.assertTrue(reports[0].with_suffix(".md").exists())
        payload = json.loads(reports[0].read_text())
        return {row["id"]: row for row in payload["results"]}


class WireFormatTests(FakeEngineCase):
    def test_frames_are_big_endian_length_prefixed_json(self) -> None:
        frame = engine_client.encode_frame({"id": "x"})
        self.assertEqual(frame[:4], len(b'{"id":"x"}').to_bytes(4, "big"))

    def test_handshake_and_typed_errors(self) -> None:
        with engine_client.EngineClient(engine_client.socket_path()) as client:
            self.assertEqual(client.handshake["protocol"], 6)
            self.assertEqual(client.computer_by_name("silo-eval-b")["configuration"]["id"], "id-b")
            with self.assertRaises(engine_client.EngineError) as raised:
                client.call("no.such.method")
            self.assertEqual(raised.exception.code, "unavailable")
            result = client.exec("id-a", ["python3", "-c", "import sys; sys.stdout.write(sys.stdin.read().upper())"], stdin=b"abc")
            self.assertEqual((result.exit_code, result.stdout), (0, b"ABC"))


class UnisolatedGuestTests(FakeEngineCase):
    """A guest that is really the host: every boundary scenario must report `fail`."""

    guest_env = {"EVAL_CANARY": "$MSB_EVAL_CANARY"}

    def test_scenarios_detect_a_guest_without_isolation(self) -> None:
        rows = self.run_evals("--peer", "silo-eval-b", "--only", "host-files", "computer-to-computer", "secret-in-guest", "engine-socket", "same-user-client")
        self.assertEqual(rows["host-files"]["verdict"], "fail", rows["host-files"])
        self.assertIn(".silo-eval-canary-", rows["host-files"]["summary"])
        self.assertEqual(rows["computer-to-computer"]["verdict"], "fail", rows["computer-to-computer"])
        self.assertEqual(rows["secret-in-guest"]["verdict"], "pass", rows["secret-in-guest"])
        self.assertEqual(rows["engine-socket"]["verdict"], "pass", rows["engine-socket"])
        self.assertEqual(rows["same-user-client"]["verdict"], "known-gap")
        self.assertEqual(self.engine.secrets, [], "the disposable secret was not removed")
        self.assertIn("computers.restart", self.engine.calls)
        self.assertEqual(list(self.home.glob(".silo-eval-canary-*")), [], "the host canary was left behind")

    def test_a_leaked_secret_value_fails(self) -> None:
        original = self.engine.handle

        def leak(method: str, params: Dict[str, Any]) -> Any:
            if method in ("guest.exec", "guest.exec.start"):
                self.engine.guest_env["EVAL_CANARY"] = getattr(self.engine, "saved_value", "")
            return original(method, params)

        self.engine.handle = leak
        rows = self.run_evals("--only", "secret-in-guest")
        self.assertEqual(rows["secret-in-guest"]["verdict"], "fail", rows["secret-in-guest"])
        self.assertNotIn(self.engine.saved_value, json.dumps(rows), "the report must not contain the secret")


class IsolatedGuestTests(FakeEngineCase):
    def test_host_files_pass_when_the_canary_is_out_of_reach(self) -> None:
        guest_root = self.home / "guest-root"
        guest_root.mkdir()
        (guest_root / "file.txt").write_text("guest data\n")
        results = self.home / "results"
        silo_eval.main(["--results", str(results), "--scan-root", str(guest_root), "--scan-min-files", "1",
                        "--app", str(self.home / "none.app"), "--only", "host-files"])
        row = json.loads(next(results.glob("*.json")).read_text())["results"][0]
        self.assertEqual(row["verdict"], "pass", row)

    def test_socket_folder_open_to_others_fails(self) -> None:
        os.chmod(self.home / ".silo", 0o755)
        rows = self.run_evals("--only", "engine-socket")
        self.assertEqual(rows["engine-socket"]["verdict"], "fail")


class SafetyTests(unittest.TestCase):
    def test_refuses_computers_not_named_for_evals(self) -> None:
        with self.assertRaises(SystemExit):
            silo_eval.require_eval_computer("dev", allow_any=False)
        silo_eval.require_eval_computer("silo-eval-a", allow_any=False)

    def test_expected_verdicts_are_valid(self) -> None:
        self.assertGreaterEqual(len(silo_eval.SCENARIOS), 5)
        self.assertLessEqual(len(silo_eval.SCENARIOS), 10)
        for scenario in silo_eval.SCENARIOS:
            self.assertIn(scenario.expected, silo_eval.VERDICTS)


if __name__ == "__main__":
    unittest.main()
