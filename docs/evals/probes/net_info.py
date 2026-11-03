"""Guest-side probe: the addresses a guest would try first to reach its host or peers."""
import json
import socket
import struct
import subprocess


def default_gateway():
    with open("/proc/net/route") as handle:
        for line in handle.readlines()[1:]:
            fields = line.split()
            if fields[1] == "00000000" and int(fields[3], 16) & 2:
                return socket.inet_ntoa(struct.pack("<L", int(fields[2], 16)))
    return None


def resolve(name):
    try:
        return sorted({info[4][0] for info in socket.getaddrinfo(name, None, socket.AF_INET)})
    except OSError as error:
        return {"error": str(error)}


def own_addresses():
    try:
        output = subprocess.run(["hostname", "-I"], capture_output=True, text=True, timeout=5).stdout
        return [a for a in output.split() if ":" not in a]
    except (OSError, subprocess.SubprocessError) as error:
        return {"error": str(error)}


with open("/etc/resolv.conf") as handle:
    nameservers = [line.split()[1] for line in handle if line.startswith("nameserver")]

json.dump({
    "gateway": default_gateway(),
    "hostMicrosandboxInternal": resolve("host.microsandbox.internal"),
    "ownAddresses": own_addresses(),
    "nameservers": nameservers,
}, __import__("sys").stdout)
