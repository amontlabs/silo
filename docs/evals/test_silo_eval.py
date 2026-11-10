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
            return {"deviceId": "fake", "name": "fake", "protocol": 8, "capabilities": [], "clientId": "client:1"}
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
            self.guest_env[request["name"]] = "$MSB_" + request["name"]
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
            self.assertEqual(client.handshake["protocol"], 7)
            self.assertEqual(client.computer_by_name("silo-eval-b")["configuration"]["id"], "id-b")
            with self.assertRaises(engine_client.EngineError) as raised:
                client.call("no.such.method")
            self.assertEqual(raised.exception.code, "unavailable")
            result = client.exec("id-a", ["python3", "-c", "import sys; sys.stdout.write(sys.stdin.read().upper())"], stdin=b"abc")
            self.assertEqual((result.exit_code, result.stdout), (0, b"ABC"))


class UnisolatedGuestTests(FakeEngineCase):
    """A guest that is really the host: every boundary scenario must report `fail`."""

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
                for name in [s["name"] for s in self.engine.secrets]:
                    self.engine.guest_env[name] = self.engine.saved_value
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


class VerdictTests(unittest.TestCase):
    """Scenario verdicts from canned guest output, for paths a fake guest cannot produce."""

    def context(self) -> "silo_eval.Context":
        ctx = silo_eval.Context(silo_eval.parse_args([]), client=None)
        ctx.computer = {"configuration": {"id": "id-a", "name": "silo-eval-a"}}
        ctx.secret_value = "real-value"
        return ctx

    def result(self, scenario_id: str) -> "silo_eval.Result":
        return silo_eval.Result(next(s for s in silo_eval.SCENARIOS if s.id == scenario_id))

    def test_a_metadata_connection_fails_host_network(self) -> None:
        ctx = self.context()

        def guest_python(_computer, probe, *argv, **_kwargs):
            if probe == "net_info.py":
                return {"gateway": "10.0.0.1", "hostMicrosandboxInternal": [], "ownAddresses": [], "nameservers": []}
            return [dict(t, connected=t["host"] in ("example.com", "169.254.169.254")) for t in json.loads(argv[0])]

        ctx.guest_python = guest_python
        result = self.result("host-network")
        silo_eval.scenario_host_network(ctx, result)
        for cleanup in ctx.cleanups:
            cleanup()
        self.assertEqual(result.verdict, "fail", result.summary)

    def test_secret_other_hosts_needs_an_echo_for_every_case(self) -> None:
        ctx = self.context()
        placeholder = "$MSB_" + ctx.secret_name
        replies = {silo_eval.ECHO_OTHER: {"status": "200", "body": placeholder},
                   silo_eval.ECHO_ALLOWED_PLAIN_HTTP: {"status": "000", "body": ""}}
        with mock.patch.object(silo_eval, "guest_curl", lambda _ctx, url, **_: dict(replies[url], url=url, stderr="", exit=0)):
            result = self.result("secret-other-hosts")
            silo_eval.scenario_secret_other_hosts(ctx, result)
            self.assertEqual(result.verdict, "inconclusive", result.summary)
            replies[silo_eval.ECHO_ALLOWED_PLAIN_HTTP] = {"status": "200", "body": placeholder}
            result = self.result("secret-other-hosts")
            silo_eval.scenario_secret_other_hosts(ctx, result)
            self.assertEqual(result.verdict, "pass", result.summary)

    def test_host_network_without_a_lan_listener_is_inconclusive(self) -> None:
        ctx = self.context()

        def guest_python(_computer, probe, *argv, **_kwargs):
            if probe == "net_info.py":
                return {"gateway": "10.0.0.1", "hostMicrosandboxInternal": [], "ownAddresses": [], "nameservers": []}
            return [dict(t, connected=t["host"] == "example.com") for t in json.loads(argv[0])]

        ctx.guest_python = guest_python
        with mock.patch.object(silo_eval, "host_lan_address", lambda: None):
            result = self.result("host-network")
            silo_eval.scenario_host_network(ctx, result)
        for cleanup in ctx.cleanups:
            cleanup()
        self.assertEqual(result.verdict, "inconclusive", result.summary)

    def test_github_grants_must_be_app_read_only_and_selected(self) -> None:
        def policy(**overrides):
            base = {"authenticationMethod": "oauth", "repositoryMode": "selected",
                    "repositories": [{"repository": "o/read", "allowPushes": False}]}
            base.update(overrides)
            return base

        check = silo_eval.github_grant_problem
        self.assertIsNone(check(policy(), "o/read", "o/other"))
        self.assertIsNotNone(check(None, "o/read", "o/other"))
        self.assertIsNotNone(check(policy(authenticationMethod="token"), "o/read", "o/other"))
        self.assertIsNotNone(check(policy(repositoryMode="all"), "o/read", "o/other"))
        self.assertIsNotNone(check(policy(repositories=[{"repository": "o/read", "allowPushes": True}]), "o/read", "o/other"))
        self.assertIsNotNone(check(policy(), "o/missing", "o/other"))
        self.assertIsNotNone(check(policy(), "o/read", "o/read"))

    def test_a_failed_sudo_is_not_a_pass(self) -> None:
        with tempfile.TemporaryDirectory() as home, mock.patch.dict(os.environ, {"HOME": home}):
            (Path(home) / ".silo").mkdir(mode=0o700)
            (Path(home) / ".silo" / "engine.sock").touch()
            ctx = silo_eval.Context(silo_eval.parse_args(["--other-user", "nobody"]), client=None)
            failed = subprocess.CompletedProcess([], 1, stdout="", stderr="sudo: a password is required")
            with mock.patch.object(silo_eval.subprocess, "run", return_value=failed):
                result = self.result("engine-socket")
                silo_eval.scenario_engine_socket(ctx, result)
        self.assertEqual(result.verdict, "inconclusive", result.summary)

    def test_each_run_uses_its_own_secret_name(self) -> None:
        first, second = self.context(), self.context()
        self.assertNotEqual(first.secret_name, second.secret_name)
        self.assertTrue(first.secret_name.startswith(silo_eval.SECRET_PREFIX + "_"))


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
