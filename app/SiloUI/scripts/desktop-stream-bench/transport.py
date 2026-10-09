#!/usr/bin/env python3
"""Measure the SSH forward a local desktop viewer uses (development only).

Starts a throwaway TCP server in the guest (bulk sender on 7901, echo on 7902),
forwards both through the computer's private SSH configuration exactly as the
viewer's tunnel does, and prints round-trip times for small messages and the
bulk throughput, idle and with bulk data flowing.

    transport.py --app "<path>/Silo Dev.app" --home ~/.silo-dev/<id> --computer e2e-stream
"""
import argparse
import json
import os
import socket
import statistics
import subprocess
import tempfile
import threading
import time
from pathlib import Path

GUEST_SERVER = r'''
import socket, threading
def bulk(c):
    chunk = b"\0" * 65536
    try:
        while True: c.sendall(chunk)
    except OSError: pass
def echo(c):
    c.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    try:
        while True:
            d = c.recv(65536)
            if not d: return
            c.sendall(d)
    except OSError: pass
def serve(port, handler):
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("127.0.0.1", port)); s.listen()
    while True:
        c, _ = s.accept(); threading.Thread(target=handler, args=(c,), daemon=True).start()
threading.Thread(target=serve, args=(7901, bulk), daemon=True).start()
serve(7902, echo)
'''


def msb(options, *command):
    contents = Path(options.app) / 'Contents'
    environment = dict(os.environ, MSB_HOME=str(Path(options.home).expanduser()),
                       MSB_PATH=str(contents / 'MacOS/msb'),
                       MSB_LIBKRUNFW_PATH=str(contents / 'Frameworks/libkrunfw.5.dylib'))
    return subprocess.run([str(contents / 'MacOS/msb'), 'exec', options.computer, '--no-start', '--', *command],
                          env=environment, capture_output=True, text=True, timeout=60)


def rtts(path, count):
    client = socket.socket(socket.AF_UNIX)
    client.settimeout(10)
    client.connect(path)
    samples = []
    for _ in range(count):
        started = time.perf_counter()
        client.sendall(b'x' * 32)
        received = 0
        while received < 32:
            data = client.recv(64)
            if not data:
                raise RuntimeError('the echo connection closed')
            received += len(data)
        samples.append((time.perf_counter() - started) * 1000)
        time.sleep(0.005)
    client.close()
    slow = [(index, round(value)) for index, value in enumerate(samples) if value > 50]
    samples.sort()
    return {'p50': round(statistics.median(samples), 2), 'p90': round(samples[int(len(samples) * 0.9)], 2),
            'max': round(samples[-1], 2), 'over50ms': slow}


def bulk(path, seconds, result):
    client = socket.socket(socket.AF_UNIX)
    client.settimeout(10)
    client.connect(path)
    received = 0
    deadline = time.perf_counter() + seconds
    started = time.perf_counter()
    while time.perf_counter() < deadline:
        data = client.recv(1 << 20)
        if not data:
            break
        received += len(data)
    result['mbps'] = round(received * 8 / (time.perf_counter() - started) / 1e6, 1)
    client.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', required=True)
    parser.add_argument('--home', required=True)
    parser.add_argument('--computer', required=True)
    parser.add_argument('--seconds', type=float, default=5)
    parser.add_argument('--echoes', type=int, default=200, help='idle round trips, 5 ms apart')
    options = parser.parse_args()
    home = Path(options.home).expanduser()
    config = home / 'ssh/desktop-viewer' / options.computer / 'ssh_config'
    alias = next(line.split()[1] for line in config.read_text().splitlines() if line.startswith('Host '))

    msb(options, 'sh', '-c', f"setsid python3 -c '{GUEST_SERVER}' >/dev/null 2>&1 & echo $! > /tmp/silo-transport.pid")
    with tempfile.TemporaryDirectory(dir=home.parent) as directory:
        bulk_socket, echo_socket = f'{directory}/b', f'{directory}/e'
        tunnel = subprocess.Popen(['/usr/bin/ssh', '-F', str(config), '-N', '-o', 'ExitOnForwardFailure=yes',
                                   '-L', f'{bulk_socket}:127.0.0.1:7901', '-L', f'{echo_socket}:127.0.0.1:7902', alias])
        try:
            for _ in range(100):
                if os.path.exists(echo_socket):
                    break
                time.sleep(0.1)
            time.sleep(1)
            report = {'idleRttMs': rtts(echo_socket, options.echoes)}
            loaded = {}
            thread = threading.Thread(target=bulk, args=(bulk_socket, options.seconds, loaded))
            thread.start()
            time.sleep(0.5)
            report['loadedRttMs'] = rtts(echo_socket, int(options.seconds * 100) // 2)
            thread.join()
            report['bulkMbps'] = loaded.get('mbps')
            print(json.dumps(report, indent=2))
        finally:
            tunnel.terminate()
            tunnel.wait()
            msb(options, 'sh', '-c', 'kill $(cat /tmp/silo-transport.pid); rm -f /tmp/silo-transport.pid')


if __name__ == '__main__':
    main()
