#!/usr/bin/env python3
"""Live check of the Silo gateway as a container's network on Linux (phase CT1).

An unprivileged user makes a user and network namespace (`unshare --user --map-root-user
--net --mount`), creates a TAP device inside it and hands the descriptor to the gateway,
which runs in the host's own namespaces, as the engine will do for a container. The
namespace holds only the TAP, so everything it reaches goes through the gateway:

  * a public HTTPS host answers through the gateway's TLS interception (the gateway CA is the
    only trust anchor the check gives curl);
  * private addresses (10.x, the host's own address, 169.254.169.254) are refused;
  * a `$MSB_<NAME>` placeholder in a header is replaced for an allowed host, and the secret
    is a fake value, so no real secret is involved;
  * DNS goes to the gateway, and an answer with a private address is blocked;
  * a port the computer publishes answers on the host's loopback.

No step needs root. Usage:

    gateway-live-check.py --gateway PATH/TO/silo-macos-gateway [--harness PATH] [--evidence-dir DIR]

With --harness, the engine's own code (silo_engine::computer_gateway, through the
gateway_tap_harness example) starts and drives the gateway; without it, this script does.

Prints one JSON document with the checks and exits non-zero if one failed. It never prints
the CA key or the fake secret.
"""

import argparse
import array
import fcntl
import json
import os
import secrets
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time

PROTOCOL_VERSION = 1
SECRET_NAME = "CT1_FAKE_TOKEN"
TAP_NAME = "tap0"
TUNSETIFF = 0x400454CA
IFF_TAP = 0x0002
IFF_NO_PI = 0x1000
PUBLISHED_GUEST_PORT = 8080
# Names that resolve, in public DNS, to a loopback address.
REBIND_NAMES = ("localtest.me", "127.0.0.1.nip.io", "127.0.0.1.sslip.io")


# ---------------------------------------------------------------------------------------
# The gateway's control protocol: a four-byte big-endian length, then JSON.


def send_message(stream, body):
    data = json.dumps(body).encode()
    stream.sendall(struct.pack(">I", len(data)) + data)


def receive_message(stream):
    (size,) = struct.unpack(">I", receive_exactly(stream, 4))
    return json.loads(receive_exactly(stream, size))


def receive_exactly(stream, count):
    data = b""
    while len(data) < count:
        chunk = stream.recv(count - len(data))
        if not chunk:
            raise RuntimeError("the gateway closed its control link")
        data += chunk
    return data


def generate_ca(gateway):
    output = subprocess.run([gateway, "--generate-ca"], check=True, capture_output=True).stdout
    (size,) = struct.unpack(">I", output[:4])
    ca = json.loads(output[4 : 4 + size])
    return ca["cert_pem"], ca["key_pem"]


def check(results, name, passed, detail):
    results.append({"check": name, "passed": bool(passed), "detail": detail})


def run(arguments, **options):
    return subprocess.run(arguments, capture_output=True, text=True, **options)


# ---------------------------------------------------------------------------------------
# Inside the namespace: the container's side.


def curl(arguments, timeout=30):
    """Runs curl; returns (exit code, HTTP status, body)."""
    result = run(
        ["curl", "-sS", "--max-time", str(timeout), "-w", "\n%{http_code}", *arguments],
        timeout=timeout + 10,
    )
    body, _, status = result.stdout.rpartition("\n")
    return result.returncode, status, body


def json_message(body):
    try:
        return json.loads(body).get("message")
    except (ValueError, AttributeError):
        return None


def dns_query(server, name, timeout=10):
    """One UDP A query to `server`; returns (rcode, [IPv4 strings])."""
    identifier = secrets.randbits(16)
    question = b"".join(bytes([len(label)]) + label.encode() for label in name.split(".")) + b"\0"
    query = struct.pack(">HHHHHH", identifier, 0x0100, 1, 0, 0, 0) + question + struct.pack(">HH", 1, 1)
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp:
        udp.settimeout(timeout)
        udp.sendto(query, (server, 53))
        reply, _ = udp.recvfrom(4096)
    answer_id, flags, questions, answers, _, _ = struct.unpack(">HHHHHH", reply[:12])
    if answer_id != identifier:
        raise RuntimeError("DNS answer for another query")
    offset = 12
    for _ in range(questions):
        while reply[offset] != 0:
            offset += reply[offset] + 1
        offset += 5
    addresses = []
    for _ in range(answers):
        if reply[offset] & 0xC0 == 0xC0:
            offset += 2
        else:
            while reply[offset] != 0:
                offset += reply[offset] + 1
            offset += 1
        rtype, _, _, length = struct.unpack(">HHIH", reply[offset : offset + 10])
        offset += 10
        if rtype == 1 and length == 4:
            addresses.append(socket.inet_ntoa(reply[offset : offset + 4]))
        offset += length
    return flags & 0xF, addresses


def configure_interface(ready):
    """The address, the route and the resolver, as the container runtime will set them."""
    guest, gateway = ready["guest_ipv4"], ready["gateway_ipv4"]
    mac = ":".join(f"{octet:02x}" for octet in ready["guest_mac"])
    for command in (
        ["ip", "link", "set", "lo", "up"],
        ["ip", "link", "set", "dev", TAP_NAME, "address", mac],
        ["ip", "link", "set", "dev", TAP_NAME, "mtu", str(ready["mtu"]), "up"],
        ["ip", "addr", "add", f"{guest}/{ready['prefix_len']}", "dev", TAP_NAME],
        ["ip", "route", "add", "default", "via", gateway],
    ):
        outcome = run(command)
        if outcome.returncode != 0:
            raise RuntimeError(f"{' '.join(command)}: {outcome.stderr.strip()}")


def inside_namespace(arguments):
    """Runs as the namespace's root; the host sees an ordinary user."""
    link = socket.socket(fileno=arguments.link_fd)
    tun = os.open("/dev/net/tun", os.O_RDWR | os.O_CLOEXEC)
    fcntl.ioctl(tun, TUNSETIFF, struct.pack("16sH22x", TAP_NAME.encode(), IFF_TAP | IFF_NO_PI))
    link.sendmsg([b"tap\n"], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", [tun]))])
    reader = link.makefile("r")
    setup = json.loads(reader.readline())
    ready = setup["ready"]
    guest = ready["guest_ipv4"]
    scratch = tempfile.mkdtemp(prefix="ct1-ns-")
    results = []

    configure_interface(ready)
    resolv = os.path.join(scratch, "resolv.conf")
    with open(resolv, "w") as handle:
        handle.write(f"nameserver {ready['dns_ipv4']}\n")
    outcome = run(["mount", "--bind", resolv, "/etc/resolv.conf"])
    if outcome.returncode != 0:
        raise RuntimeError(f"bind-mounting resolv.conf: {outcome.stderr.strip()}")
    ca = os.path.join(scratch, "gateway-ca.pem")
    with open(ca, "w") as handle:
        handle.write(ready["ca_cert_pem"])

    interfaces = sorted(os.listdir("/sys/class/net"))
    check(results, "the namespace holds only a loopback and the TAP", interfaces == ["lo", TAP_NAME], interfaces)

    server = subprocess.Popen(
        [sys.executable, "-m", "http.server", str(PUBLISHED_GUEST_PORT), "--bind", guest],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        cwd=scratch,
    )
    try:
        for _ in range(50):  # up when the namespace itself can connect to it
            try:
                socket.create_connection((guest, PUBLISHED_GUEST_PORT), timeout=1).close()
                break
            except OSError:
                time.sleep(0.1)
        link.sendall(b'{"event":"listening"}\n')
        reader.readline()  # the host has tried the published port

        # 1. A public HTTPS host answers through the gateway's interception.
        code, status, _ = curl(["--cacert", ca, "-o", "/dev/null", "https://example.com/"])
        check(results, "a public HTTPS host answers through the gateway", code == 0 and status == "200", {"curl": code, "status": status})
        code, _, _ = curl(["-o", "/dev/null", "https://example.com/"])
        check(results, "without the gateway CA the connection is not trusted (the gateway signed it)", code == 60, {"curl": code})
        code, status, _ = curl(["-o", "/dev/null", "http://example.com/"])
        check(results, "public plain HTTP answers", code == 0 and status[:1] in ("2", "3"), {"curl": code, "status": status})

        # 2. Private addresses are refused.
        for label, url in (
            ("a 10.x address", "http://10.255.255.1/"),
            ("another private range", "http://192.168.0.1/"),
            ("the link-local metadata address", "http://169.254.169.254/latest/meta-data/"),
            ("the host's own address", f"http://{setup['host_ip']}:{setup['host_listener_port']}/"),
            ("the host's own address over TLS", f"https://{setup['host_ip']}:{setup['host_listener_port']}/"),
        ):
            code, status, _ = curl(["-k", "--connect-timeout", "5", "-o", "/dev/null", url], timeout=8)
            check(results, f"{label} is refused", code != 0 and status in ("000", ""), {"curl": code, "status": status})

        # 3. A placeholder is replaced for an allowed host and nowhere else.
        header = f"Authorization: Bearer $MSB_{SECRET_NAME}"
        code, status, body = curl(["--cacert", ca, "-H", header, "https://api.github.com/user"])
        check(
            results,
            "api.github.com answers a substituted fake token with Bad credentials",
            code == 0 and status == "401" and "Bad credentials" in body,
            {"curl": code, "status": status, "message": json_message(body)},
        )
        code, status, body = curl(["--cacert", ca, "https://api.github.com/user"])
        check(
            results,
            "api.github.com answers a request without a token with Requires authentication",
            code == 0 and status == "401" and "Requires authentication" in body,
            {"curl": code, "status": status, "message": json_message(body)},
        )
        curl(["--cacert", ca, "-H", header, "-o", "/dev/null", "https://example.com/"])
        # The request may pass or be blocked; the secret is substituted only for its own host,
        # which the credential-use log (checked at the end) shows.

        # 4. DNS goes to the gateway; a private answer is blocked.
        rcode, addresses = dns_query(ready["dns_ipv4"], "example.com")
        check(results, "a DNS query is answered by the gateway", rcode == 0 and bool(addresses), {"rcode": rcode, "addresses": len(addresses)})
        rebound = []
        for name in setup["rebind_names"]:
            try:
                rcode, addresses = dns_query(ready["dns_ipv4"], name)
            except (OSError, RuntimeError) as error:
                rebound.append({"name": name, "outcome": f"no answer ({type(error).__name__})"})
                continue
            rebound.append({"name": name, "rcode": rcode, "private_answer": any(a.startswith("127.") for a in addresses)})
        check(
            results,
            "a DNS answer with a private address is blocked",
            bool(rebound) and not any(item.get("private_answer") for item in rebound),
            rebound,
        )
    finally:
        server.terminate()
        server.wait()
    link.sendall((json.dumps({"event": "results", "results": results}) + "\n").encode())


# ---------------------------------------------------------------------------------------
# On the host: what the engine does.


def host_address():
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        probe.connect(("192.0.2.1", 9))  # no packet is sent
        return probe.getsockname()[0]


def resolves_privately(name):
    try:
        return socket.gethostbyname(name).startswith("127.")
    except OSError:
        return False


def tool_version(command):
    try:
        return run(command).stdout.splitlines()[0]
    except (OSError, IndexError):
        return None


def high_descriptor(fd):
    return fcntl.fcntl(fd, fcntl.F_DUPFD_CLOEXEC, 10)


def start_gateway(gateway, tap_fd, control_fd, usage_log):
    """fd 3 is the TAP and fd 4 the control link, as the supervisor hands them over. Returns
    the process id; no other descriptor is passed on."""
    tap_high, control_high = high_descriptor(tap_fd), high_descriptor(control_fd)
    try:
        return os.posix_spawn(
            gateway,
            [gateway],
            dict(os.environ, SILO_SECRET_USAGE_LOG=usage_log),
            file_actions=[
                (os.POSIX_SPAWN_DUP2, tap_high, 3),
                (os.POSIX_SPAWN_DUP2, control_high, 4),
                (os.POSIX_SPAWN_OPEN, 0, os.devnull, os.O_RDONLY, 0),
            ],
        )
    finally:
        os.close(tap_high)
        os.close(control_high)


def alive(pid):
    try:
        return os.waitpid(pid, os.WNOHANG) == (0, 0)
    except ChildProcessError:
        return False


GUEST_MAC = [0x02, 0x53, 0x49, 0x4C, 0x4F, 0x01]
GUEST_IPV4 = "172.30.0.6"
ALLOWED_HOSTS = ["api.github.com"]


class DirectGateway:
    """The gateway binary driven over its control protocol by this script."""

    def __init__(self, gateway, tap_fd, usage_log, fake_value):
        cert_pem, key_pem = generate_ca(gateway)
        self.control, control_child = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
        self.pid = start_gateway(gateway, tap_fd, control_child.fileno(), usage_log)
        control_child.close()
        send_message(self.control, {
            "type": "init", "version": PROTOCOL_VERSION, "profile": "container",
            "slot": 1, "guest_ipv4": GUEST_IPV4, "guest_mac": GUEST_MAC, "mtu": 1500,
            "dns_nameservers": [], "intercepted_ports": [443],
            "loopback_ports": [{"host_port": 0, "guest_port": PUBLISHED_GUEST_PORT, "protocol": "tcp"}],
            "ca": {"cert_pem": cert_pem, "key_pem": key_pem},
            "secrets": [{
                "name": SECRET_NAME, "value": fake_value,
                "placeholder": f"$MSB_{SECRET_NAME}", "allowed_hosts": ALLOWED_HOSTS,
            }],
            "github_profile_json": None,
        })
        self.ready = receive_message(self.control)
        if self.ready.get("type") != "ready":
            raise RuntimeError(f"the gateway did not become ready: {self.ready.get('code', self.ready.get('type'))}")

    def stats(self):
        send_message(self.control, {"type": "stats", "version": PROTOCOL_VERSION})
        return receive_message(self.control).get("counters", {})

    def shutdown(self):
        """Returns (stopped as asked, exit code)."""
        send_message(self.control, {"type": "shutdown", "version": PROTOCOL_VERSION})
        stopped = receive_message(self.control).get("type") == "stopped"
        _, status = os.waitpid(self.pid, 0)
        self.pid = None
        return stopped, os.waitstatus_to_exitcode(status)

    def abort(self):
        if self.pid is not None and alive(self.pid):
            os.kill(self.pid, signal.SIGTERM)
            os.waitpid(self.pid, 0)


class HarnessGateway:
    """The gateway started by the engine's own code (silo_engine::computer_gateway), through
    the `gateway_tap_harness` example."""

    def __init__(self, harness, gateway, tap_fd, usage_log, fake_value):
        self.process = subprocess.Popen(
            [harness, "--gateway", gateway, "--tap-fd", str(tap_fd)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, pass_fds=[tap_fd],
        )
        self.process.stdin.write(json.dumps({
            "slot": 1, "guest_ipv4": GUEST_IPV4, "guest_mac": GUEST_MAC, "mtu": 1500,
            "published_ports": [PUBLISHED_GUEST_PORT],
            "secrets": [{"name": SECRET_NAME, "value": fake_value, "allowed_hosts": ALLOWED_HOSTS}],
            "usage_log": usage_log,
        }) + "\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        if not line:
            raise RuntimeError(f"the engine did not start the gateway (exit {self.process.wait()})")
        self.ready = json.loads(line)

    def stats(self):
        self.process.stdin.write("stats\n")
        self.process.stdin.flush()
        return json.loads(self.process.stdout.readline())

    def shutdown(self):
        self.process.stdin.write("shutdown\n")
        self.process.stdin.close()
        code = self.process.wait(timeout=30)
        return code == 0, code

    def abort(self):
        if self.process.poll() is None:
            self.process.terminate()
            self.process.wait(timeout=10)


def on_host(arguments):
    gateway = os.path.abspath(arguments.gateway)
    scratch = tempfile.mkdtemp(prefix="ct1-host-")
    usage_log = os.path.join(scratch, "secret-usage.log")
    fake_value = "ghp_" + "".join(secrets.choice("abcdefghijklmnopqrstuvwxyz0123456789") for _ in range(36))

    # A listener on every address of the host, to show that nothing reaches it.
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.bind(("0.0.0.0", 0))
    listener.listen(8)
    accepted = []

    def accept_loop():
        while True:
            try:
                connection, peer = listener.accept()
            except OSError:
                return
            accepted.append(peer)
            connection.close()

    threading.Thread(target=accept_loop, daemon=True).start()

    link_host, link_child = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
    namespace = subprocess.Popen(
        [
            "unshare", "--user", "--map-root-user", "--net", "--mount", "--",
            sys.executable, os.path.abspath(__file__), "--in-namespace", "--link-fd", str(link_child.fileno()),
        ],
        pass_fds=[link_child.fileno()],
    )
    link_child.close()
    backend = None
    try:
        data, ancillary, _, _ = link_host.recvmsg(16, socket.CMSG_LEN(struct.calcsize("i")))
        if data != b"tap\n" or not ancillary:
            raise RuntimeError("the namespace did not hand over a TAP descriptor")
        received = array.array("i")
        received.frombytes(ancillary[0][2][: struct.calcsize("i")])
        tap_fd = received[0]

        if arguments.harness:
            backend = HarnessGateway(os.path.abspath(arguments.harness), gateway, tap_fd, usage_log, fake_value)
        else:
            backend = DirectGateway(gateway, tap_fd, usage_log, fake_value)
        os.close(tap_fd)
        ready = backend.ready
        setup = {
            "ready": ready, "host_ip": host_address(), "host_listener_port": listener.getsockname()[1],
            "rebind_names": [name for name in REBIND_NAMES if resolves_privately(name)],
        }
        link_host.sendall((json.dumps(setup) + "\n").encode())
        reader = link_host.makefile("r")
        if json.loads(reader.readline())["event"] != "listening":
            raise RuntimeError("the namespace did not start its listener")

        results = []
        published = ready["loopback_ports"][0]["host_port"]
        try:
            with socket.create_connection(("127.0.0.1", published), timeout=10) as connection:
                connection.sendall(b"GET / HTTP/1.0\r\n\r\n")
                reply = connection.recv(64)
        except OSError as error:
            reply = type(error).__name__.encode()
        check(results, "a published port answers on the host's loopback", reply.startswith(b"HTTP/"), {"reply_start": reply[:12].decode(errors="replace")})
        link_host.sendall(b'{"event":"go"}\n')
        results += json.loads(reader.readline())["results"]
        namespace.wait(timeout=60)

        check(results, "nothing from the namespace reached a listener on the host's address", not accepted, {"connections": len(accepted)})
        check(results, "the check had names that resolve to a private address to test with", bool(setup["rebind_names"]), setup["rebind_names"])
        counters = backend.stats()
        names = ("guest_to_engine_frames", "engine_to_guest_frames", "guest_mac_spoof", "guest_ip_spoof",
                 "malformed_frames", "ipv6_rejected", "backend_write_errors", "frame_send_errors")
        check(
            results,
            "frames crossed the TAP in both directions",
            counters.get("guest_to_engine_frames", 0) > 0 and counters.get("engine_to_guest_frames", 0) > 0,
            {name: counters.get(name) for name in names},
        )
        try:
            with open(usage_log) as handle:
                usage = handle.read()
        except OSError:
            usage = ""
        check(
            results,
            "the credential-use log names the secret, the host and the method, and never the value",
            SECRET_NAME in usage and "api.github.com" in usage and fake_value not in usage,
            {"lines": len(usage.splitlines()), "first_line": usage.splitlines()[0] if usage else None},
        )
        check(
            results,
            "the secret was used for api.github.com only, not for a request to another host",
            "example.com" not in usage and usage.count('"kind":"used"') == usage.count("api.github.com"),
            {"used_events": usage.count('"kind":"used"')},
        )
        stopped, exit_code = backend.shutdown()
        backend = None
        check(results, "the gateway stops when asked and exits cleanly", stopped and exit_code == 0, {"stopped": stopped, "exit": exit_code})
        check(results, "the check ran without root", os.geteuid() != 0, f"euid {os.geteuid()}")

        document = {
            "ok": all(item["passed"] for item in results),
            "started_by": "the engine (silo_engine::computer_gateway)" if arguments.harness else "the script, over the control protocol",
            "kernel": os.uname().release,
            "machine": os.uname().machine,
            "curl": tool_version(["curl", "--version"]),
            "unshare": tool_version(["unshare", "--version"]),
            "ip": tool_version(["ip", "-V"]),
            "checks": results,
        }
        text = json.dumps(document, indent=2)
        print(text)
        if arguments.evidence_dir:
            os.makedirs(arguments.evidence_dir, exist_ok=True)
            with open(os.path.join(arguments.evidence_dir, "results.json"), "w") as handle:
                handle.write(text + "\n")
        return 0 if document["ok"] else 1
    finally:
        listener.close()
        if backend is not None:
            backend.abort()
        if namespace.poll() is None:
            namespace.terminate()
            try:
                namespace.wait(timeout=10)
            except subprocess.TimeoutExpired:
                namespace.kill()
        try:
            os.unlink(usage_log)
        except OSError:
            pass


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--gateway", help="path to the gateway binary")
    parser.add_argument("--harness", help="path to the gateway_tap_harness example: the engine starts the gateway")
    parser.add_argument("--evidence-dir")
    parser.add_argument("--in-namespace", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--link-fd", type=int, help=argparse.SUPPRESS)
    arguments = parser.parse_args()
    if arguments.in_namespace:
        inside_namespace(arguments)
        return 0
    if not arguments.gateway:
        parser.error("--gateway is required")
    return on_host(arguments)


if __name__ == "__main__":
    sys.exit(main())
