#!/usr/bin/env python3
"""Disposable Ubuntu 24.04 Selkies/Xfce fixture. Never production infrastructure."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import platform
import pwd
import re
import secrets
import shutil
import signal
import socket
import stat
import subprocess
import sys
import time
import urllib.request

ROOT = Path("/var/lib/silo-desktop-probe")
RUN = Path("/run/silo-desktop-probe")
LOG = Path("/var/log/silo-desktop-probe")
MANIFEST = Path(__file__).with_name("desktop-viewer-probe-lock.json")
MARKER_KIND = "silo-desktop-viewer-probe"
SCREEN = "4096x4096x24"
START_SIZE = "1440x900"
START_MODELINE = ["88.75", "1440", "1488", "1520", "1600", "900", "903", "909", "926", "+hsync", "-vsync"]
PORT = 6901
USER = "silo"

def fail(message):
    raise RuntimeError(message)

def read_marker(path):
    path = Path(path)
    fd = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
            fail("scratch marker must be a root-owned regular file, not group/world writable")
        value = json.loads(os.read(fd, 65536))
    finally:
        os.close(fd)
    if not isinstance(value, dict):
        fail("scratch marker must contain a JSON object")
    if (value.get("kind") != MARKER_KIND or value.get("scratchOnly") is not True
            or not isinstance(value.get("fixtureId"), str) or len(value["fixtureId"]) < 8
            or not isinstance(value.get("baselineStopped"), bool)):
        fail("scratch marker must set kind, scratchOnly, fixtureId, and baselineStopped")
    return value

def preflight(marker_path, require_stopped=False):
    marker = read_marker(marker_path)
    if sys.platform != "linux" or os.geteuid() != 0:
        fail("run this fixture only as root inside a Linux guest")
    release = {}
    for line in Path("/etc/os-release").read_text().splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            release[key] = value.strip('"')
    if release.get("ID") != "ubuntu" or release.get("VERSION_ID") != "24.04":
        fail("fixture supports Ubuntu 24.04 only")
    try:
        account = pwd.getpwnam(USER)
    except KeyError:
        fail("existing silo account is required")
    if account.pw_uid == 0 or account.pw_dir != "/home/silo" or not Path(account.pw_dir).is_dir():
        fail("existing silo account must be non-root with home /home/silo")
    if require_stopped and marker["baselineStopped"] is not True:
        fail("The computer owner must stop the baseline desktop and set baselineStopped=true in the scratch marker")
    return marker, account

def boot_id():
    return Path("/proc/sys/kernel/random/boot_id").read_text().strip()

def proc_identity(pid):
    try:
        raw = Path(f"/proc/{pid}/stat").read_text()
        tail = raw[raw.rfind(")") + 2:].split()
        return {"startTicks": int(tail[19]), "pgid": int(tail[2]),
                "uid": Path(f"/proc/{pid}").stat().st_uid, "exe": os.readlink(f"/proc/{pid}/exe")}
    except (OSError, ValueError, IndexError):
        return None

def state_read():
    try:
        return json.loads((RUN / "state.json").read_text())
    except (OSError, json.JSONDecodeError):
        return None

def state_write(value):
    RUN.mkdir(mode=0o755, parents=True, exist_ok=True)
    temporary = RUN / "state.json.tmp"
    temporary.write_text(json.dumps(value, sort_keys=True))
    temporary.chmod(0o600)
    temporary.replace(RUN / "state.json")

def port_open():
    with socket.socket() as probe:
        probe.settimeout(.2)
        return probe.connect_ex(("127.0.0.1", PORT)) == 0

def validate_live_marker(marker):
    state = state_read()
    if not state or state.get("fixtureId") != marker["fixtureId"]:
        fail("no fixture state matches this scratch marker")

def locked_asset(machine):
    manifest = json.loads(MANIFEST.read_text())
    arch = {"x86_64": "amd64", "aarch64": "arm64"}.get(machine)
    if not arch or arch not in manifest["assets"]:
        fail(f"unsupported guest architecture: {machine}")
    return manifest, arch, manifest["assets"][arch]

def install(marker_path):
    marker, _ = preflight(marker_path)
    manifest, arch, asset = locked_asset(platform.machine())
    cache = ROOT / "downloads"
    cache.mkdir(mode=0o700, parents=True, exist_ok=True)
    package = cache / f"selkies-{manifest['version']}-{arch}.deb"
    if not package.exists() or hashlib.sha256(package.read_bytes()).hexdigest() != asset["sha256"]:
        temporary = package.with_suffix(".part")
        digest = hashlib.sha256()
        with urllib.request.urlopen(asset["url"], timeout=90) as response, temporary.open("wb") as output:
            while block := response.read(1024 * 1024):
                output.write(block)
                digest.update(block)
        if digest.hexdigest() != asset["sha256"]:
            temporary.unlink(missing_ok=True)
            fail("Selkies release asset SHA-256 mismatch")
        temporary.replace(package)
        package.chmod(0o600)
    subprocess.run(["apt-get", "install", "-y", "--no-install-recommends",
                    "xvfb", "pulseaudio", "dbus-x11", "xauth", str(package)], check=True)
    for command in ("Xvfb", "pulseaudio", "dbus-run-session", "xauth", "startxfce4", "selkies"):
        if not shutil.which(command):
            fail(f"fixture dependency missing after install: {command}")
    print(json.dumps({"installed": True, "version": manifest["version"], "architecture": arch,
                      "fixtureId": marker["fixtureId"]}))

def refuse_conflicts():
    if Path("/tmp/.X11-unix/X1").exists() or Path("/tmp/.X1-lock").exists():
        fail("display :1 is present; inspect it and stop the baseline desktop explicitly")
    if port_open():
        fail("guest loopback port 6901 is occupied; inspect it and stop the baseline desktop explicitly")
    with socket.socket() as probe:
        if probe.connect_ex(("127.0.0.1", 5901)) == 0:
            fail("baseline VNC port 5901 is still listening")

def reap_spawned_child(child):
    # Popen retains the exact child PID. Signal only that child, never its group.
    if child.poll() is not None:
        return
    child.terminate()
    try:
        child.wait(timeout=3)
    except subprocess.TimeoutExpired:
        if child.poll() is None:
            child.kill()
        child.wait(timeout=3)

def spawn(name, argv, env, account):
    LOG.mkdir(mode=0o700, parents=True, exist_ok=True)
    log = (LOG / f"{name}.log").open("ab", buffering=0)
    def demote():
        os.initgroups(USER, account.pw_gid)
        os.setgid(account.pw_gid)
        os.setuid(account.pw_uid)
    try:
        child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                                 env=env, start_new_session=True, preexec_fn=demote, close_fds=True)
    finally:
        log.close()
    try:
        time.sleep(.25)
        identity = proc_identity(child.pid)
        if child.poll() is not None or not identity or identity["uid"] != account.pw_uid or identity["pgid"] != child.pid:
            fail(f"{name} exited or failed identity validation; inspect {LOG / (name + '.log')}")
        return {"name": name, "pid": child.pid, "startTicks": identity["startTicks"],
                "uid": identity["uid"], "pgid": identity["pgid"], "exe": identity["exe"]}
    except BaseException:
        reap_spawned_child(child)
        raise

def set_start_size(env, account):
    def xrandr(*arguments):
        return subprocess.run(["xrandr", *arguments], env=env, capture_output=True, text=True, timeout=10,
                              preexec_fn=lambda: (os.initgroups(USER, account.pw_gid),
                                                  os.setgid(account.pw_gid), os.setuid(account.pw_uid)))
    query = xrandr("--query")
    output = re.search(r"^(\S+) connected", query.stdout, re.MULTILINE)
    if query.returncode != 0 or not output:
        fail("xrandr found no Xvfb output; check x11-xserver-utils and the RANDR extension")
    if not re.search(rf"^\s+{START_SIZE}\s", query.stdout, re.MULTILINE):
        xrandr("--newmode", START_SIZE, *START_MODELINE)
        added = xrandr("--addmode", output.group(1), START_SIZE)
        if added.returncode != 0:
            fail(f"xrandr could not add {START_SIZE}: {added.stderr.strip()}")
    applied = xrandr("--output", output.group(1), "--mode", START_SIZE, "--fb", START_SIZE)
    if applied.returncode != 0:
        fail(f"xrandr could not set {START_SIZE}: {applied.stderr.strip()}")

def session_environment(account):
    session = RUN / "session"
    return {"HOME": account.pw_dir, "USER": USER, "LOGNAME": USER, "PATH": os.environ["PATH"],
            "DISPLAY": ":1", "XAUTHORITY": str(session / "Xauthority"), "XDG_RUNTIME_DIR": str(session),
            "PULSE_RUNTIME_PATH": str(session / "pulse"),
            "PULSE_SERVER": f"unix:{session / 'pulse' / 'native'}"}

def selkies_args(secret):
    return ["selkies", "--addr=127.0.0.1", f"--port={PORT}", "--enable-https=false",
            "--basic-auth-user=silo", f"--basic-auth-password={secret}", "--encoder=h264enc",
            "--use-cpu=true", "--mode=websockets", "--enable-dual-mode=false|locked", "--enable-resize=true", "--use-css-scaling=true|locked",
            "--enable-clipboard=true", "--enable-binary-clipboard=true", "--clipboard-seamless=false",
            "--file-transfers=none", "--audio-enabled=true", "--audio-bitrate=64000",
            "--microphone-enabled=false|locked", "--ui-sidebar-show-audio-settings=false"]

def stop_recorded(item):
    current = proc_identity(item["pid"])
    if not current:
        return
    if (current["startTicks"] != item["startTicks"] or current["uid"] != item["uid"]
            or current["pgid"] != item["pgid"] or current["pgid"] != item["pid"]
            or current["exe"] != item["exe"]):
        fail(f"refusing to signal changed process identity for {item['name']} PID {item['pid']}")
    os.killpg(item["pgid"], signal.SIGTERM)
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline and proc_identity(item["pid"]):
        time.sleep(.1)
    if proc_identity(item["pid"]):
        fail(f"{item['name']} did not stop cleanly; no forced kill attempted")

def start(marker_path):
    marker, account = preflight(marker_path, require_stopped=True)
    existing = state_read()
    if existing and existing.get("processes"):
        fail("fixture state exists; run stop first")
    if not Path("/opt/selkies").exists() or not shutil.which("selkies"):
        fail("run install before start")
    for command in ("startxfce4", "Xvfb", "pulseaudio", "dbus-run-session", "xauth"):
        if not shutil.which(command):
            fail(f"required baseline command is missing: {command}")
    refuse_conflicts()
    session = RUN / "session"
    session.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chown(session, account.pw_uid, account.pw_gid)
    os.chmod(session, 0o700)
    authority = session / "Xauthority"
    cookie = secrets.token_hex(16)
    subprocess.run(["runuser", "-u", USER, "--", "xauth", "-f", str(authority),
                    "add", ":1", "MIT-MAGIC-COOKIE-1", cookie], check=True)
    (session / "pulse").mkdir(mode=0o700, exist_ok=True)
    os.chown(session / "pulse", account.pw_uid, account.pw_gid)
    env = session_environment(account)
    secret = secrets.token_hex(32)
    ROOT.mkdir(mode=0o700, parents=True, exist_ok=True)
    (ROOT / "connection.json").write_text(json.dumps({"username": "silo", "password": secret, "port": PORT}))
    (ROOT / "connection.json").chmod(0o600)
    state = {"fixtureId": marker["fixtureId"], "bootId": boot_id(), "processes": []}
    state_write(state)
    commands = [
        ("xvfb", ["Xvfb", ":1", "-screen", "0", SCREEN, "+extension", "RANDR", "-noreset",
                          "-nolisten", "tcp", "-auth", str(authority)]),
        ("pulse", ["pulseaudio", "--daemonize=no", "--exit-idle-time=-1"]),
        ("xfce", ["dbus-run-session", "--", "startxfce4"]),
        ("selkies", selkies_args(secret)),
    ]
    for name, argv in commands:
        state["processes"].append(spawn(name, argv, env, account))
        state_write(state)
        time.sleep(.5)
        if name == "xvfb":
            set_start_size(env, account)
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline and not port_open():
        time.sleep(.2)
    if not port_open():
        fail(f"Selkies not listening on 127.0.0.1:{PORT}; inspect {LOG / 'selkies.log'}; run stop")
    print(json.dumps({"state": "running", "display": ":1", "resolution": "1440x900",
                      "listener": f"127.0.0.1:{PORT}", "fixtureId": marker["fixtureId"]}))

def stop(marker_path):
    marker, _ = preflight(marker_path)
    validate_live_marker(marker)
    state = state_read()
    if state.get("bootId") != boot_id():
        state["processes"] = []
    else:
        for item in reversed(state.get("processes", [])):
            stop_recorded(item)
        state["processes"] = []
    state_write(state)
    print(json.dumps({"state": "stopped", "fixtureId": marker["fixtureId"]}))

def process_identities_ready(state):
    expected = {"xvfb", "pulse", "xfce", "selkies"}
    if not isinstance(state, dict):
        return False
    processes = state.get("processes")
    if (not isinstance(processes, list) or len(processes) != len(expected)
            or not all(isinstance(item, dict) for item in processes)
            or {item.get("name") for item in processes} != expected):
        return False
    if state.get("bootId") != boot_id():
        return False
    try:
        expected_uid = pwd.getpwnam(USER).pw_uid
    except KeyError:
        return False
    for item in processes:
        current = proc_identity(item.get("pid"))
        if (not current or current["startTicks"] != item.get("startTicks")
                or current["uid"] != item.get("uid") or current["uid"] != expected_uid
                or current["pgid"] != item.get("pgid") or current["pgid"] != item.get("pid")
                or current["exe"] != item.get("exe")):
            return False
    return True

def stream_ready(state):
    if not process_identities_ready(state):
        return False
    try:
        credentials = json.loads((ROOT / "connection.json").read_text())
        if credentials.get("port") != PORT or not credentials.get("username") or not credentials.get("password"):
            return False
        token = base64.b64encode(f"{credentials['username']}:{credentials['password']}".encode()).decode()
        request = urllib.request.Request(f"http://127.0.0.1:{PORT}/",
                                         headers={"Authorization": f"Basic {token}"})
        with urllib.request.urlopen(request, timeout=2) as response:
            content_type = response.headers.get("Content-Type", "").lower()
            return response.status == 200 and "text/html" in content_type and bool(response.read(4096))
    except (OSError, ValueError, KeyError, urllib.error.URLError):
        return False

def restart_streamer(marker_path):
    marker, account = preflight(marker_path, require_stopped=True)
    validate_live_marker(marker)
    state = state_read()
    if not process_identities_ready(state) or not stream_ready(state):
        fail("fixture processes and stream must be healthy before restarting Selkies")
    streamer = next(item for item in state["processes"] if item["name"] == "selkies")
    credentials = json.loads((ROOT / "connection.json").read_text())
    secret = credentials.get("password")
    if (credentials.get("username") != USER or credentials.get("port") != PORT
            or not isinstance(secret, str) or len(secret) != 64
            or any(char not in "0123456789abcdef" for char in secret)):
        fail("fixture connection credentials are invalid")
    session = RUN / "session"
    if not (session / "Xauthority").is_file() or not (session / "pulse").is_dir():
        fail("fixture X11/audio session files are missing; no process was changed")
    unaffected = [item.copy() for item in state["processes"] if item["name"] != "selkies"]
    stop_recorded(streamer)
    state["processes"] = [item.copy() for item in unaffected]
    state_write(state)
    replacement = spawn("selkies", selkies_args(secret), session_environment(account), account)
    state["processes"].append(replacement)
    state_write(state)
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline and not stream_ready(state):
        time.sleep(.2)
    if not stream_ready(state):
        fail(f"restarted Selkies is not ready; inspect {LOG / 'selkies.log'}; run stop")
    if [item for item in state["processes"] if item["name"] != "selkies"] != unaffected:
        fail("fixture desktop process records changed during Selkies restart")
    print(json.dumps({"state": "running", "streamerRestarted": True,
                      "fixtureId": marker["fixtureId"]}))

def status(marker_path):
    marker, _ = preflight(marker_path)
    validate_live_marker(marker)
    state = state_read()
    print(json.dumps({"installed": True, "version": "selkies-probe-2.0.0",
                      "state": "running" if stream_ready(state) else "stopped",
                      "autoStart": False, "port": PORT, "user": USER, "display": ":1"}))

def connection(marker_path):
    marker, _ = preflight(marker_path)
    validate_live_marker(marker)
    print((ROOT / "connection.json").read_text())

def make_shim(marker_path):
    marker, _ = preflight(marker_path, require_stopped=True)
    validate_live_marker(marker)
    target = ROOT / "shim" / "silo-desktop"
    target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    script_path = str(Path(__file__).resolve())
    marker_path = str(Path(marker_path).resolve())
    source = (
        "#!/usr/bin/env python3\n"
        "# SCRATCH-ONLY helper. The computer owner may install it after preserving the baseline helper.\n"
        "import runpy, sys\n"
        f"script = {script_path!r}\n"
        f"marker = {marker_path!r}\n"
        "if len(sys.argv) != 2 or sys.argv[1] not in (\"status\", \"connection\"):\n"
        "    raise SystemExit(\"scratch shim supports only status|connection\")\n"
        "sys.argv = [script, sys.argv[1], \"--marker\", marker]\n"
        "runpy.run_path(script, run_name=\"__main__\")\n"
    )
    target.write_text(source)
    target.chmod(0o755)
    print(str(target))

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("install", "start", "stop", "restart-streamer", "status", "connection", "shim"))
    parser.add_argument("--marker", required=True, help="computer-owner-created root-owned scratch authorization JSON")
    args = parser.parse_args()
    {"install": install, "start": start, "stop": stop, "restart-streamer": restart_streamer, "status": status,
     "connection": connection, "shim": make_shim}[args.action](args.marker)

if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(f"desktop-viewer-probe: {error}", file=sys.stderr)
        raise SystemExit(1)
