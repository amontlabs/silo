#!/usr/bin/env bash
# T5a live check: Silo's tools in a computer over its own loopback port and token.
# Runs against the Dev engine (~/.silo-dev) with throwaway e2e-t5-* computers. Run:
#   bash docs/direction/checks/t5_channel_check.sh 2>&1 | tee docs/direction/evidence/T5a.txt
# Needs a Dev app bundle built from this tree (BUNDLE), its debug `silo` (SILO) and a guest `silo`
# built with app/SiloUI/scripts/build-guest-silo.sh (GUEST). The script starts the bundle's
# `silod --agent` by hand when no Dev engine runs, restarts it once mid-way, and stops it at the end.
# It never touches another engine: it refuses to run when a Dev engine it did not start answers.
#
# What it shows, in order:
#   S1  a new Linux computer has the endpoint file, and `silo list` inside it shows only itself
#   S2  from inside, it forks itself, runs a command in the fork, reads its diff, stops and drops it
#   S3  a call that needs approval answers with an approval id; `silo approvals allow` on the device
#       runs it; the computer reads the outcome
#   S4  a second computer cannot reach the first one's port; the first one's token is refused on the
#       second's port; a program on the device without the token gets nothing; a fork's copy of the
#       token is refused and the fork has its own
#   S5  an owner-only call is refused
#   S6  `silo mcp` inside the computer lists its tools (without the device-only ones) and answers a call
#   S7  the audit log shows the calls with the computer as caller
#   S8  the engine restarts and the channel works again on the same port
#   S9  a memory checkpoint is restored and the client reconnects
#   S10 turning the tools off closes an open session at once
#   S11 a computer whose tools were off gets them after `silo tools enable` and a restart
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
T=${T:-$HOME/code/projects/silo-wt-targets/T5a}
SILO=${SILO:-$T/debug/silo}
GUEST=${GUEST:-$T/guest/aarch64-unknown-linux-musl/release/silo}
BUNDLE=${BUNDLE:-$T/debug/bundle/macos/Silo Dev.app}
SILOD="$BUNDLE/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod"
LOG=${LOG:-$REPO/app/SiloUI/src-tauri/target/verification/silod-dev-t5a.log}
SOCK="$HOME/.silo-dev/engine.sock"
A=e2e-t5-a; B=e2e-t5-b; C=e2e-t5-c; F=e2e-t5-af; G=e2e-t5-ag
WORK=$(mktemp -d "${TMPDIR:-/tmp}/e2e-t5.XXXXXX")
PASS=0; FAILN=0; STARTED=""
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
# In a computer: as its `silo` account, or as root.
# The device's `silo exec` adds a "step N" line of its own; it is left out.
inside() { local c=$1; shift; "$SILO" exec "$c" --timeout 170 -- sudo -u silo -H bash -c "$*" 2>&1 | sed -u '/^step [0-9]*$/d'; return "${PIPESTATUS[0]}"; }
root() { local c=$1; shift; "$SILO" exec "$c" --timeout 170 -- sudo bash -c "$*" 2>&1 | sed -u '/^step [0-9]*$/d'; return "${PIPESTATUS[0]}"; }
our_engine() { pgrep -f "^$SILOD --agent" | head -1; }
engine_answers() { "$SILO" list >/dev/null 2>&1; }
start_engine() {
  "$SILOD" --agent >>"$LOG" 2>&1 &
  STARTED=$!
  for _ in $(seq 1 120); do engine_answers && return 0; sleep 1; done
  return 1
}
stop_engine() {
  local pid; pid=$(our_engine); [ -n "$pid" ] || return 0
  kill -TERM "$pid"
  for _ in $(seq 1 180); do kill -0 "$pid" 2>/dev/null || return 0; sleep 1; done
  return 1
}
engine() { python3 -I "$WORK/engine.py" "$SOCK" "$@"; }
status_port() { "$SILO" --json tools status "$1" 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin).get("port",""))'; }
names_inside() { inside "$1" 'silo --json list' 2>&1 | python3 -c '
import json,sys
text=sys.stdin.read()
try:
    v,_=json.JSONDecoder().raw_decode(text.lstrip())
except Exception:
    print("ERROR: "+text.strip().replace("\n"," ")[:300]); sys.exit()
rows=v if isinstance(v,list) else v.get("computers",[])
print(" ".join(sorted(r.get("name","?") for r in rows)))'; }

cleanup() {
  say "cleanup (drops only the e2e-t5-* computers this run made, then stops the Dev engine it started)"
  if engine_answers; then
    for c in $F $G $A $B $C; do "$SILO" stop "$c" >/dev/null 2>&1; done
    for round in 1 2 3; do
      for c in $F $G $A $B $C; do "$SILO" status "$c" >/dev/null 2>&1 && "$SILO" drop "$c" --force >/dev/null 2>&1; done
      sleep 2
    done
    say "silo list after cleanup:"; "$SILO" list
  fi
  stop_engine && say "Dev engine stopped" || say "Dev engine did not stop in time"
  rm -rf "$WORK"
}
trap cleanup EXIT

# A raw client of the channel, run in a computer or on the device: connects, sends a handshake
# with the token given ("-" for none) and says whether anything answered. `hold` keeps an
# answered session open and reports when it ends.
cat >"$WORK/probe.py" <<'PY'
import json, socket, struct, sys, time
host, port, token = sys.argv[1], int(sys.argv[2]), sys.argv[3]
mode = sys.argv[4] if len(sys.argv) > 4 else "once"
start = time.time()
try:
    s = socket.create_connection((host, port), timeout=5)
except Exception as error:
    print("CONNECT_FAIL", type(error).__name__); sys.exit(0)
params = {"protocol": {"min": 8, "max": 8}}
if token != "-":
    params["token"] = token
body = json.dumps({"id": "h", "method": "engine.handshake", "params": params}).encode()
s.sendall(struct.pack(">I", len(body)) + body)
s.settimeout(None if mode == "hold" else 8)
def frame():
    head = b""
    while len(head) < 4:
        chunk = s.recv(4 - len(head))
        if not chunk:
            return None
        head += chunk
    (n,) = struct.unpack(">I", head)
    data = b""
    while len(data) < n:
        data += s.recv(n - len(data))
    return json.loads(data)
try:
    reply = frame()
except socket.timeout:
    print("NO_ANSWER_TIMEOUT"); sys.exit(0)
except ConnectionResetError:
    reply = None
if reply is None:
    print("CLOSED_NO_ANSWER after %.1fs" % (time.time() - start)); sys.exit(0)
print("ANSWERED protocol=%s" % reply.get("result", {}).get("protocol"), flush=True)
if mode == "hold":
    held = time.time()
    try:
        while s.recv(4096):
            pass
    except Exception:
        pass
    print("SESSION_ENDED after %.1fs" % (time.time() - held), flush=True)
PY
# The device's engine socket, for what the CLI has no command for (audit, restore).
cat >"$WORK/engine.py" <<PY
import json, sys
sys.path.insert(0, "$REPO/docs/evals")
from engine_client import EngineClient
sock, op = sys.argv[1], sys.argv[2]
with EngineClient(sock, client_name="t5-check") as e:
    if op == "audit":
        rows = e.call("audit.list", {"callerKind": "computer", "limit": 200})["entries"]
        for r in [r for r in rows if r.get("callerId") == sys.argv[3]]:
            print("%-10s %-14s %-34s %-9s %-20s count=%s" % (r.get("callerKind"), (r.get("callerId") or "-")[:12], r.get("method"), r.get("outcome"), r.get("errorCode") or "", r.get("count", 1)))
    elif op == "restore":
        job = e.call("checkpoints.restore", {"computerId": sys.argv[3], "checkpointId": sys.argv[4]})["jobId"]
        print(json.dumps(e.wait_job(job, timeout=600))[:300])
PY
B64_PROBE=$(base64 <"$WORK/probe.py" | tr -d '\n')
put_probe() { root "$1" "echo $B64_PROBE | base64 -d > /usr/local/bin/t5probe.py && chmod 755 /usr/local/bin/t5probe.py"; }
field() { root "$1" "python3 -c 'import json; print(json.load(open(\"/etc/silo/endpoint.json\"))[\"$2\"])'" 2>/dev/null | tr -d '\r'; }

say "== header"
echo "commit $(cd "$REPO" && git rev-parse --short HEAD) on $(cd "$REPO" && git branch --show-current); $("$SILO" --version 2>&1); host $(sw_vers -productVersion) $(uname -m); $(date -u +%FT%TZ)"
echo "guest silo: $(file -b "$GUEST" | cut -c1-60)"
if engine_answers && [ -z "$(our_engine)" ]; then
  echo "A Dev engine this script did not start is answering; refusing to touch it." >&2
  trap - EXIT; rm -rf "$WORK"; exit 2
fi
[ -n "$(our_engine)" ] || { start_engine; check "the Dev engine starts from the bundle" $? "pid $(our_engine), log $LOG"; }
echo "computers before: $("$SILO" list 2>/dev/null | awk 'NR>1{print $1}' | tr '\n' ' ')"
for c in $A $B $C $F $G; do "$SILO" status "$c" >/dev/null 2>&1 && { echo "$c already exists; refusing to run" >&2; exit 2; }; done

say "== S1 a new Linux computer has the endpoint file and sees only itself"
for c in $A $B; do
  "$SILO" create $c --linux --cpus 1 --memory 2 --disk 8 --no-desktop >/dev/null 2>&1 && "$SILO" start $c >/dev/null 2>&1
  check "create and start $c" $? ""
done
echo "status $A: $("$SILO" tools status $A 2>&1)"
for c in $A $B; do
  out=$("$SILO" tools enable $c --binary "$GUEST" 2>&1); check "silo tools enable $c --binary" $? "$out"
done
PORT_A=$(status_port $A); PORT_B=$(status_port $B)
echo "ports: $A=$PORT_A $B=$PORT_B"
ls_out=$(root $A 'stat -c "%U:%G %a" /etc/silo/endpoint.json' 2>&1 | tr -d '\r')
check "S1 endpoint file is root's, group silo, 0640" $(echo "$ls_out" | grep -qx "root:silo 640"; echo $?) "$ls_out"
FILE_PORT=$(field $A port); FILE_HOST=$(field $A host); TOKEN_A=$(field $A token)
check "S1 file names host.microsandbox.internal and the computer's port" $([ "$FILE_HOST" = host.microsandbox.internal ] && [ "$FILE_PORT" = "$PORT_A" ]; echo $?) "host=$FILE_HOST port=$FILE_PORT token=${TOKEN_A:0:8}... (${#TOKEN_A} chars)"
seen=$(names_inside $A)
check "S1 silo list inside $A shows only $A" $([ "$seen" = "$A" ]; echo $?) "$seen"

say "== S2 from inside: fork itself, run in the fork, diff, stop, drop"
out=$(inside $A "silo fork $A --name $F" 2>&1); check "S2 fork $A from inside" $? "$(echo "$out" | tail -2 | tr '\n' ' ')"
out=$(inside $A "silo start $F" 2>&1); check "S2 start the fork from inside" $? "$(echo "$out" | tail -1)"
out=$(inside $A "silo exec $F -- echo hello-from-the-fork" 2>&1); echo "$out" | grep -q hello-from-the-fork; check "S2 exec in the fork" $? "$out"
out=$(inside $A "silo diff $F" 2>&1); check "S2 diff of the fork" $? "$(echo "$out" | head -3 | tr '\n' ' ')"
seen=$(names_inside $A)
check "S2 silo list inside shows $A and its fork" $([ "$seen" = "$A $F" ]; echo $?) "$seen"
out=$(inside $A "silo stop $F && silo drop $F" 2>&1); check "S2 stop and drop the fork from inside" $? "$(echo "$out" | tail -1)"
"$SILO" status $F >/dev/null 2>&1; check "S2 the fork is gone on the device" $([ $? -ne 0 ]; echo $?) ""

say "== S3 a call that needs approval"
out=$(inside $A "silo publish $A 8080" 2>&1); rc=$?
ID=$(echo "$out" | grep -oE 'ap-[0-9a-f]{16}' | head -1)
check "S3 publish from inside answers with an approval id" $([ $rc -ne 0 ] && [ -n "$ID" ]; echo $?) "$out"
out=$("$SILO" approvals allow "$ID" 2>&1); check "S3 silo approvals allow on the device" $? "$out"
sleep 2
out=$(inside $A "silo approvals show $ID" 2>&1); echo "$out" | sed 's/^/  inside: /' | head -12
echo "$out" | grep -qi "allowed\|ran\|done\|8080"; check "S3 the computer reads the outcome" $? ""
out=$("$SILO" ports $A 2>&1); echo "$out" | grep -q 8080; check "S3 port 8080 is published" $? "$(echo "$out" | tr '\n' ' ')"
# `silo unpublish` then reads the ports with network.state, which is the owner's; the device checks.
out=$(inside $A "silo unpublish $A 8080" 2>&1); echo "  inside: $out"
out=$("$SILO" ports $A 2>&1); check "S3 unpublish from inside (narrowing) needed no approval" $(! echo "$out" | grep -q 8080; echo $?) "$(echo "$out" | tr '\n' ' ')"

say "== S4 isolation of the port and the token"
put_probe $A; put_probe $B
out=$(inside $B "python3 /usr/local/bin/t5probe.py host.microsandbox.internal $PORT_A $TOKEN_A" 2>&1)
check "S4 $B cannot connect to $A's port" $(echo "$out" | grep -q CONNECT_FAIL; echo $?) "$out"
out=$(inside $B "python3 /usr/local/bin/t5probe.py host.microsandbox.internal $PORT_B $TOKEN_A" 2>&1)
check "S4 $A's token is refused on $B's port, after a delay" $(echo "$out" | grep -qE 'CLOSED_NO_ANSWER after (0\.9|[1-9])'; echo $?) "$out"
out=$(inside $A "python3 /usr/local/bin/t5probe.py host.microsandbox.internal $PORT_A -" 2>&1)
check "S4 no token on $A's own port gets no answer" $(echo "$out" | grep -q CLOSED_NO_ANSWER; echo $?) "$out"
out=$(inside $A "python3 /usr/local/bin/t5probe.py host.microsandbox.internal $PORT_A $TOKEN_A" 2>&1)
check "S4 control: $A's token on $A's port is answered" $(echo "$out" | grep -q 'ANSWERED protocol=8'; echo $?) "$out"
out=$(python3 -I "$WORK/probe.py" 127.0.0.1 "$PORT_A" silo_gc_guess 2>&1)
check "S4 a program on the device without the token gets nothing" $(echo "$out" | grep -q CLOSED_NO_ANSWER; echo $?) "$out"
out=$("$SILO" fork $A --name $G 2>&1); check "S4 fork $A on the device" $? "$(echo "$out" | tail -1)"
"$SILO" start $G >/dev/null 2>&1
for _ in $(seq 1 30); do [ -n "$(field $G token)" ] && break; sleep 2; done
TOKEN_G=$(field $G token); PORT_G=$(status_port $G)
check "S4 the fork has its own port and token" $([ -n "$TOKEN_G" ] && [ "$TOKEN_G" != "$TOKEN_A" ] && [ "$PORT_G" != "$PORT_A" ]; echo $?) "port $PORT_G, token differs: $([ "$TOKEN_G" != "$TOKEN_A" ] && echo yes || echo no)"
out=$(inside $G "python3 /usr/local/bin/t5probe.py host.microsandbox.internal $PORT_G $TOKEN_A" 2>&1)
check "S4 a copy of $A's token in the fork is refused on the fork's port" $(echo "$out" | grep -q CLOSED_NO_ANSWER; echo $?) "$out"
out=$(inside $G "python3 /usr/local/bin/t5probe.py host.microsandbox.internal $PORT_A $TOKEN_A" 2>&1)
check "S4 the fork cannot reach $A's port" $(echo "$out" | grep -q CONNECT_FAIL; echo $?) "$out"
"$SILO" stop $G >/dev/null 2>&1; "$SILO" drop $G --force >/dev/null 2>&1

say "== S5 an owner-only call is refused"
out=$(inside $A "silo secrets list" 2>&1); rc=$?
check "S5 silo secrets list inside is refused" $([ $rc -ne 0 ]; echo $?) "$out"
out=$(inside $A "silo approvals allow $ID" 2>&1); rc=$?
check "S5 a computer cannot answer an approval" $([ $rc -ne 0 ]; echo $?) "$out"

say "== S6 silo mcp inside the computer"
cp "$REPO/docs/v1/checks/phase5_mcp_client.py" "$WORK/mcp_client.py"
echo '[{"list": true}, {"call": "list", "args": {}}]' >"$WORK/steps.json"
B64_MCP=$(base64 <"$WORK/mcp_client.py" | tr -d '\n'); B64_STEPS=$(base64 <"$WORK/steps.json" | tr -d '\n')
root $A "echo $B64_MCP | base64 -d > /usr/local/bin/t5mcp.py; echo $B64_STEPS | base64 -d > /tmp/t5steps.json; chmod 644 /tmp/t5steps.json" >/dev/null
out=$(inside $A "cd /tmp && python3 /usr/local/bin/t5mcp.py silo /tmp /tmp/t5steps.json" 2>&1)
echo "$out" | cut -c1-600 | sed 's/^/  mcp: /'
echo "$out" | grep -q '"tools"' && ! echo "$out" | grep -qE '"(review|ssh_name|request_open)"'; check "S6 tools listed without the device-only ones" $? ""
echo "$out" | grep '"call": "list"' | grep -q "$A"; check "S6 the list call answers with $A" $? ""

say "== S7 the audit log names the computer as caller"
ID_A=$("$SILO" --json status $A 2>/dev/null | python3 -c 'import json,sys; v=json.load(sys.stdin); print(v.get("id") or v.get("configuration",{}).get("id",""))')
rows=$(engine audit "$ID_A" 2>&1); echo "$rows" | head -40 | sed 's/^/  /'
echo "$rows" | grep -E '^computer' | grep -q 'checkpoints.fork'; check "S7 the fork made from inside is recorded as $A's" $? ""
echo "$rows" | grep -E '^computer' | grep -q 'network.publish.*asked'; check "S7 the publish is recorded as asked" $? ""
echo "$rows" | grep -E '^computer' | grep -q 'engine.handshake.*refused'; check "S7 the refused handshakes are recorded" $? ""

say "== S8 the engine restarts and the channel works again"
stop_engine; check "S8 the Dev engine stops" $? ""
start_engine; check "S8 the Dev engine starts again" $? "pid $(our_engine)"
"$SILO" start $A >/dev/null 2>&1
P2=$(status_port $A); check "S8 $A keeps its port" $([ "$P2" = "$PORT_A" ]; echo $?) "$P2"
seen=""; for _ in $(seq 1 20); do seen=$(names_inside $A); [ "$seen" = "$A" ] && break; sleep 3; done
check "S8 silo list inside works after the restart" $([ "$seen" = "$A" ]; echo $?) "$seen"

say "== S9 a memory checkpoint restore, and the client reconnects"
"$SILO" checkpoint $A --name t5mem >/dev/null 2>&1
CP=$("$SILO" --json checkpoints $A 2>/dev/null | python3 -c '
import json,sys
v=json.load(sys.stdin); rows=v if isinstance(v,list) else v.get("checkpoints",[])
for r in rows:
    if r.get("name")=="t5mem": print(r.get("id",""), r.get("scope",""))')
echo "  checkpoint: $CP"
check "S9 a checkpoint with memory of the running computer" $(echo "$CP" | grep -q " full$"; echo $?) "$CP"
CP=${CP%% *}
out=$(engine restore "$ID_A" "$CP" 2>&1); echo "  restore: $(echo "$out" | cut -c1-200)"
# A restored computer waits for its next start, which resumes the saved memory.
"$SILO" start $A >/dev/null 2>&1; check "S9 start $A from its restored memory" $? ""
seen=""; for _ in $(seq 1 20); do seen=$(names_inside $A); [ "$seen" = "$A" ] && break; sleep 3; done
check "S9 silo list inside works after the restore" $([ "$seen" = "$A" ]; echo $?) "$seen"

say "== S10 turning the tools off closes an open session"
TOKEN_A=$(field $A token)
( inside $A "python3 /usr/local/bin/t5probe.py host.microsandbox.internal $PORT_A $TOKEN_A hold" >"$WORK/hold.txt" 2>&1 ) &
HOLD=$!
for _ in $(seq 1 20); do grep -q ANSWERED "$WORK/hold.txt" && break; sleep 1; done
out=$("$SILO" tools disable $A 2>&1); check "S10 silo tools disable" $? "$out"
for _ in $(seq 1 20); do grep -q SESSION_ENDED "$WORK/hold.txt" && break; sleep 1; done
wait $HOLD 2>/dev/null
check "S10 the open session ended" $(grep -q SESSION_ENDED "$WORK/hold.txt"; echo $?) "$(tr '\n' ' ' <"$WORK/hold.txt")"
out=$(inside $A "silo list" 2>&1); rc=$?
check "S10 silo inside no longer reaches Silo" $([ $rc -ne 0 ]; echo $?) "$(echo "$out" | tail -1)"

say "== S11 an existing computer gets the tools after enable and a restart"
"$SILO" create $C --linux --cpus 1 --memory 2 --disk 8 --no-desktop >/dev/null 2>&1; check "create $C" $? ""
"$SILO" tools disable $C >/dev/null 2>&1
"$SILO" start $C >/dev/null 2>&1
echo "  $("$SILO" tools status $C 2>&1)"
out=$("$SILO" tools enable $C --binary "$GUEST" 2>&1); check "S11 silo tools enable $C while it runs" $? "$out"
echo "$out" | grep -q "restart it"; check "S11 the answer says a restart is needed" $? ""
out=$(inside $C "silo list" 2>&1); rc=$?
check "S11 before the restart the computer cannot reach its port" $([ $rc -ne 0 ]; echo $?) "$(echo "$out" | tail -1)"
"$SILO" stop $C >/dev/null 2>&1; "$SILO" start $C >/dev/null 2>&1; check "S11 restart $C" $? ""
echo "  $("$SILO" tools status $C 2>&1)"
seen=""; for _ in $(seq 1 20); do seen=$(names_inside $C); [ "$seen" = "$C" ] && break; sleep 3; done
check "S11 silo list inside works after the restart" $([ "$seen" = "$C" ]; echo $?) "$seen"

say "== engine log lines about the channels"
grep -E "channel of|endpoint file" "$LOG" | tail -25 | sed 's/^/  /'
say "== summary: $PASS passed, $FAILN failed"
