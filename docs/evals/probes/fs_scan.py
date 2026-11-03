"""Guest-side probe: list mounts and search the guest's files for host-only values.

Run inside a Silo computer as `python3 - <hex-needles-json> [root]` with this file on stdin.
Needles arrive hex-encoded so that the search pattern itself never appears in plain
text on the guest's filesystem or in a process command line, which would otherwise
make the scan find its own arguments.

Prints one JSON object: mounts, the files that contain any needle, files that look
like they hold a real GitHub token, and how much was read.
"""
import json
import os
import re
import stat
import sys

SKIP_TREES = ("/proc", "/sys", "/dev")
CHUNK = 1 << 20
MAX_FILE_BYTES = 512 << 20
GITHUB_TOKEN = re.compile(rb"\b(gh[opsu]_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{60,})\b")


def mounts():
    rows = []
    with open("/proc/self/mountinfo", "rb") as handle:
        for line in handle:
            fields = line.decode("utf-8", "replace").split()
            separator = fields.index("-")
            rows.append({
                "mountPoint": fields[4],
                "options": fields[5],
                "fsType": fields[separator + 1],
                "source": fields[separator + 2],
            })
    return rows


def scan_file(path, needles, report):
    try:
        info = os.lstat(path)
    except OSError:
        report["unreadable"] += 1
        return
    if not stat.S_ISREG(info.st_mode):
        return
    longest = max(len(n) for n in needles.values())
    tail = b""
    read = 0
    try:
        with open(path, "rb") as handle:
            while read < MAX_FILE_BYTES:
                chunk = handle.read(CHUNK)
                if not chunk:
                    break
                read += len(chunk)
                window = tail + chunk
                for label, needle in needles.items():
                    if needle in window:
                        report["hits"].append({"path": path, "needle": label})
                for match in GITHUB_TOKEN.finditer(window):
                    token = match.group(0)
                    report["githubTokenLike"].append({
                        "path": path,
                        "redacted": token[:7].decode() + "…" + str(len(token)),
                    })
                tail = window[-(longest + 96):]
    except OSError:
        report["unreadable"] += 1
        return
    report["files"] += 1
    report["bytes"] += read


def scan_process_memory_views(needles, report):
    """Environment and command line of every guest process, which the tree walk skips."""
    own = {str(os.getpid()), str(os.getppid())}
    for pid in os.listdir("/proc"):
        if not pid.isdigit() or pid in own:
            continue
        for leaf in ("environ", "cmdline"):
            path = "/proc/%s/%s" % (pid, leaf)
            try:
                with open(path, "rb") as handle:
                    data = handle.read()
            except OSError:
                continue
            for label, needle in needles.items():
                if needle in data:
                    report["hits"].append({"path": path, "needle": label})
            for match in GITHUB_TOKEN.finditer(data):
                token = match.group(0)
                report["githubTokenLike"].append({"path": path, "redacted": token[:7].decode() + "…" + str(len(token))})


def main():
    needles = {label: bytes.fromhex(value) for label, value in json.loads(sys.argv[1]).items()}
    report = {"mounts": mounts(), "hits": [], "githubTokenLike": [], "files": 0, "bytes": 0, "unreadable": 0}
    top = sys.argv[2] if len(sys.argv) > 2 else "/"
    for root, directories, files in os.walk(top, topdown=True, followlinks=False):
        if root == "/":
            directories[:] = [d for d in directories if "/" + d not in SKIP_TREES]
        for name in files:
            scan_file(os.path.join(root, name), needles, report)
    scan_process_memory_views(needles, report)
    json.dump(report, sys.stdout)


main()
