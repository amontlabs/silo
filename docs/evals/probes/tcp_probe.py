"""Guest-side probe: try TCP connections and report which ones a listener answered.

argv[1] is a JSON list of {"label", "host", "port", "send"}. Each target gets a fresh
connection with a short timeout; `send` is written as one line and up to 256 bytes of
the reply are kept, so the host can tell its own listener answered.
"""
import json
import socket
import sys

results = []
for target in json.loads(sys.argv[1]):
    row = {"label": target["label"], "host": target["host"], "port": target["port"], "connected": False}
    try:
        with socket.create_connection((target["host"], target["port"]), timeout=4) as connection:
            row["connected"] = True
            connection.settimeout(4)
            connection.sendall((target.get("send", "") + "\n").encode())
            try:
                row["reply"] = connection.recv(256).decode("utf-8", "replace")
            except OSError as error:
                row["replyError"] = str(error)
    except OSError as error:
        row["error"] = "%s: %s" % (type(error).__name__, error)
    results.append(row)
json.dump(results, sys.stdout)
