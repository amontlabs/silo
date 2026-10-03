import importlib.util
import json
from pathlib import Path
import tempfile
import types
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("desktop-viewer-probe-guest.py")
SPEC = importlib.util.spec_from_file_location("desktop_viewer_probe_guest", SCRIPT)
PROBE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROBE)


class RestartStreamerRecordsTest(unittest.TestCase):
    def test_restart_replaces_only_the_selkies_record(self):
        original = [
            {"name": name, "pid": pid, "startTicks": pid * 10, "uid": 1001,
             "pgid": pid, "exe": f"/usr/bin/{name}"}
            for name, pid in (("xvfb", 10), ("pulse", 11), ("xfce", 12), ("selkies", 13))
        ]
        state = {"fixtureId": "fixture-12345678", "processes": original}
        replacement = {"name": "selkies", "pid": 14, "startTicks": 140, "uid": 1001,
                       "pgid": 14, "exe": "/usr/bin/selkies"}

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            run = root / "run"
            (run / "session" / "pulse").mkdir(parents=True)
            (run / "session" / "Xauthority").touch()
            (root / "connection.json").write_text(json.dumps(
                {"username": "silo", "password": "a" * 64, "port": 6901}))
            writes = []
            with (patch.object(PROBE, "ROOT", root), patch.object(PROBE, "RUN", run),
                  patch.object(PROBE, "preflight", return_value=({"fixtureId": "fixture-12345678"}, object())),
                  patch.object(PROBE, "validate_live_marker"),
                  patch.object(PROBE, "state_read", return_value=state.copy()),
                  patch.object(PROBE, "state_write", side_effect=lambda value: writes.append(value.copy())),
                  patch.object(PROBE, "process_identities_ready", return_value=True),
                  patch.object(PROBE, "stream_ready", return_value=True),
                  patch.object(PROBE, "stop_recorded"),
                  patch.object(PROBE, "spawn", return_value=replacement),
                  patch.object(PROBE, "session_environment", return_value={}),
                  patch.object(PROBE, "selkies_args", return_value=[])):
                PROBE.restart_streamer("marker")

        final_records = writes[-1]["processes"]
        self.assertEqual(
            [item for item in final_records if item["name"] != "selkies"], original[:3])
        self.assertEqual([item for item in final_records if item["name"] == "selkies"], [replacement])


def xrandr_result(returncode=0, stdout="", stderr=""):
    return types.SimpleNamespace(returncode=returncode, stdout=stdout, stderr=stderr)


QUERY_BEFORE = "Screen 0: minimum 1 x 1, current 4096 x 4096, maximum 4096 x 4096\nscreen connected\n"
QUERY_AFTER = "Screen 0: minimum 1 x 1, current 1440 x 900, maximum 4096 x 4096\nscreen connected\n   1440x900 60.00*\n"


class StartSizeReadinessTest(unittest.TestCase):
    ACCOUNT = types.SimpleNamespace(pw_gid=1000)

    def run_size(self, responses, query_seconds=0.0, **options):
        calls = []
        timeouts = []
        pending = list(responses)
        now = [0.0]

        def fake_run(command, **keywords):
            calls.append(command[1:])
            timeouts.append(keywords["timeout"])
            now[0] += query_seconds
            return pending.pop(0) if len(pending) > 1 else pending[0]

        def sleep(seconds):
            now[0] += seconds

        try:
            with patch.object(PROBE.subprocess, "run", side_effect=fake_run):
                PROBE.set_start_size({}, self.ACCOUNT, sleep=sleep, clock=lambda: now[0], **options)
        finally:
            self.elapsed = now[0]
            self.timeouts = timeouts
        return calls

    def test_waits_for_a_display_that_is_transiently_unavailable(self):
        calls = self.run_size([
            xrandr_result(1, "", "can't open display"), xrandr_result(1, "", "can't open display"),
            xrandr_result(0, QUERY_BEFORE), xrandr_result(), xrandr_result(), xrandr_result(),
            xrandr_result(0, QUERY_AFTER)])
        self.assertEqual(calls[2], ["--query"])
        self.assertIn(["--output", "screen", "--mode", "1440x900", "--fb", "1440x900"], calls)

    def test_gives_up_after_the_readiness_bound(self):
        with self.assertRaisesRegex(RuntimeError, "no Xvfb output"):
            self.run_size([xrandr_result(1, "", "can't open display")], ready_seconds=1)

    def test_slow_queries_cannot_stretch_the_readiness_window(self):
        with self.assertRaisesRegex(RuntimeError, "no Xvfb output"):
            self.run_size([xrandr_result(1, "", "can't open display")], query_seconds=4.0, ready_seconds=10)
        self.assertLessEqual(self.elapsed, 10 + 4.0)
        self.assertLessEqual(max(self.timeouts), 10)
        self.assertLess(self.timeouts[-1], 4.0 + 0.001)

    def test_a_query_that_times_out_is_retried_within_the_window(self):
        now = [0.0]

        def hung(command, **keywords):
            now[0] += keywords["timeout"]
            raise PROBE.subprocess.TimeoutExpired(command, keywords["timeout"])

        with patch.object(PROBE.subprocess, "run", side_effect=hung):
            with self.assertRaisesRegex(RuntimeError, "no Xvfb output"):
                PROBE.set_start_size({}, self.ACCOUNT, ready_seconds=10,
                                     sleep=lambda seconds: now.__setitem__(0, now[0] + seconds), clock=lambda: now[0])
        self.assertLessEqual(now[0], 10.001)

    def test_fails_when_the_applied_size_is_not_reported(self):
        with self.assertRaisesRegex(RuntimeError, "did not change"):
            self.run_size([xrandr_result(0, QUERY_BEFORE), xrandr_result(), xrandr_result(),
                           xrandr_result(), xrandr_result(0, QUERY_BEFORE)])

    def test_start_stops_recorded_processes_when_the_size_cannot_be_set(self):
        record = {"name": "xvfb", "pid": 10}
        writes = []
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            run = root / "run"
            with (patch.object(PROBE, "ROOT", root), patch.object(PROBE, "RUN", run),
                  patch.object(PROBE, "LOG", root / "log"),
                  patch.object(PROBE, "preflight", return_value=({"fixtureId": "f"}, types.SimpleNamespace(pw_uid=0, pw_gid=0, pw_dir="/"))),
                  patch.object(PROBE, "state_read", return_value=None),
                  patch.object(PROBE, "state_write", side_effect=lambda value: writes.append(json.loads(json.dumps(value)))),
                  patch.object(PROBE, "boot_id", return_value="boot"),
                  patch.object(PROBE, "refuse_conflicts"), patch.object(PROBE.Path, "exists", return_value=True),
                  patch.object(PROBE.shutil, "which", return_value="/usr/bin/x"),
                  patch.object(PROBE.os, "chown"), patch.object(PROBE.os, "chmod"),
                  patch.object(PROBE.subprocess, "run"),
                  patch.object(PROBE, "spawn", return_value=record),
                  patch.object(PROBE, "set_start_size", side_effect=RuntimeError("no output")),
                  patch.object(PROBE, "stop_recorded") as stop,
                  patch.object(PROBE.time, "sleep")):
                with self.assertRaisesRegex(RuntimeError, "no output"):
                    PROBE.start("marker")
        stop.assert_called_once_with(record)
        self.assertEqual(writes[-1]["processes"], [])


if __name__ == "__main__":
    unittest.main()
