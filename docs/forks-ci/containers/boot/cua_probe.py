"""Starts `cua-driver mcp`, lists its tools, and calls a screenshot tool if there is one."""
import json
import os
import select
import subprocess
import sys
import time

env = dict(os.environ, CUA_DRIVER_RS_TELEMETRY_ENABLED='0', CUA_DRIVER_RS_UPDATE_CHECK='false')
proc = subprocess.Popen(['/usr/local/bin/cua-driver', 'mcp'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE, env=env, text=True, bufsize=1)


def send(message):
    proc.stdin.write(json.dumps(message) + '\n')
    proc.stdin.flush()


def receive(wanted_id, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        ready, _, _ = select.select([proc.stdout], [], [], 0.5)
        if not ready:
            if proc.poll() is not None:
                return None
            continue
        line = proc.stdout.readline()
        if not line:
            return None
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if message.get('id') == wanted_id:
            return message
    return None


start = time.monotonic()
send({'jsonrpc': '2.0', 'id': 1, 'method': 'initialize',
      'params': {'protocolVersion': '2024-11-05', 'capabilities': {}, 'clientInfo': {'name': 'c1', 'version': '0'}}})
init = receive(1, 30)
print('CUA initialize', 'ok' if init and 'result' in init else f'failed {init}', f'{time.monotonic() - start:.2f}s')
if not init:
    print('CUA stderr', (proc.stderr.read(2000) if proc.poll() is not None else 'driver still running, no answer'))
    sys.exit(1)
send({'jsonrpc': '2.0', 'method': 'notifications/initialized'})
send({'jsonrpc': '2.0', 'id': 2, 'method': 'tools/list'})
tools = receive(2, 30)
names = [t['name'] for t in (tools or {}).get('result', {}).get('tools', [])]
print('CUA tools', len(names), ' '.join(names)[:300])
shot = next((n for n in names if 'screenshot' in n), None)
if shot:
    begin = time.monotonic()
    send({'jsonrpc': '2.0', 'id': 3, 'method': 'tools/call', 'params': {'name': shot, 'arguments': {}}})
    reply = receive(3, 60)
    content = (reply or {}).get('result', {}).get('content', [])
    kinds = [c.get('type') for c in content]
    sizes = [len(c.get('data', '')) for c in content if c.get('type') == 'image']
    error = (reply or {}).get('result', {}).get('isError') or (reply or {}).get('error')
    print(f'CUA call {shot} content={kinds} image_base64_bytes={sizes} error={str(error)[:200]} {time.monotonic() - begin:.2f}s')
else:
    print('CUA call none: no tool with "screenshot" in its name')
proc.terminate()
