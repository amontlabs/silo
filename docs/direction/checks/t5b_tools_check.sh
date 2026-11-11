#!/usr/bin/env bash
# T5b live check: the engine puts the `silo` program into a computer and registers its MCP server
# with the agents, without anyone passing a binary.
# Runs against the Dev engine (~/.silo-dev) with throwaway e2e-t5b-* computers. Run:
#   bash docs/direction/checks/t5b_tools_check.sh 2>&1 | tee docs/direction/evidence/T5b.txt
# Needs a Dev app bundle built from this tree (BUNDLE: `npm --prefix app/SiloUI run
# desktop:build:debug` after `npm --prefix app/SiloUI run runtime:prepare`) and its debug `silo`
# (SILO). The script starts the bundle's `silod --agent` by hand when no Dev engine runs, and stops
# it at the end. It never touches another engine: it refuses to run when a Dev engine it did not
# start answers.
#
# What it shows, in order:
#   S1  a new Linux computer has /usr/local/bin/silo with the bundled file's checksum and a receipt,
#       nobody passing a binary; `silo list` works inside; the status lists the components
#   S2  agents.install (Claude Code) registers the `silo` MCP server beside the computer-use one; an MCP
#       client started inside the computer gets a tool list from `silo mcp`
#   S3  a fork has its own endpoint and the registration works against the fork
#   S4  turning the tools off removes the registration (also for a computer that is stopped at the
#       time), keeps the program, and the program refuses
#   S5  a deleted or replaced program comes back at the next start
#   S6  a reconcile that fails (the file cannot be replaced) leaves the computer running and shows
#       the failure in its status; the next start after the fix recovers
#   S7  `silo tools enable --binary` goes through the same engine path; the next start puts the
#       shipped program back
#   S8  macOS computers on the Dev engine
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
T=${T:-$HOME/code/projects/silo-wt-targets/T5b}
SILO=${SILO:-$T/debug/silo}
BUNDLE=${BUNDLE:-$T/debug/bundle/macos/Silo Dev.app}
SILOD="$BUNDLE/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod"
GUEST_MANIFEST="$BUNDLE/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/Resources/guest-support/silo/manifest.json"
GUEST_FILE="$BUNDLE/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/Resources/guest-support/silo/silo"
LOG=${LOG:-$REPO/app/SiloUI/src-tauri/target/verification/silod-dev-t5b.log}
SOCK="$HOME/.silo-dev/engine.sock"
A=e2e-t5b-a; F=e2e-t5b-af
WORK=$(mktemp -d "${TMPDIR:-/tmp}/e2e-t5b.XXXXXX")
PASS=0; FAILN=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
inside() { local c=$1; shift; "$SILO" exec "$c" --timeout 170 -- sudo -u silo -H bash -c "$*" 2>&1 | sed -u '/^step [0-9]*$/d'; return "${PIPESTATUS[0]}"; }
root() { local c=$1; shift; "$SILO" exec "$c" --timeout 170 -- sudo bash -c "$*" 2>&1 | sed -u '/^step [0-9]*$/d'; return "${PIPESTATUS[0]}"; }
our_engine() { pgrep -f "^$SILOD --agent" | head -1; }
engine_answers() { "$SILO" list >/dev/null 2>&1; }
start_engine() {
  mkdir -p "$(dirname "$LOG")"
  "$SILOD" --agent >>"$LOG" 2>&1 &
  for _ in $(seq 1 120); do engine_answers && return 0; sleep 1; done
  return 1
}
stop_engine() {
  local pid; pid=$(our_engine); [ -n "$pid" ] || return 0
  kill -TERM "$pid"
  for _ in $(seq 1 180); do kill -0 "$pid" 2>/dev/null || return 0; sleep 1; done
  return 1
}
# Polls a command until it succeeds, at most $1 seconds.
wait_for() { local limit=$1; shift; local end=$((SECONDS+limit)); while [ $SECONDS -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 3; done; return 1; }
engine() { python3 -I "$WORK/engine.py" "$SOCK" "$@"; }
tools_status() { "$SILO" tools status "$1" 2>&1; }
component_state() { "$SILO" --json tools status "$1" 2>/dev/null | python3 -c '
import json,sys
v=json.load(sys.stdin)
for c in v.get("components",[]):
    if c["id"]==sys.argv[1]: print(c["state"]); break' "$2"; }
restart() { "$SILO" stop "$1" >/dev/null 2>&1; "$SILO" start "$1" >/dev/null 2>&1; }
sha_inside() { root "$1" 'sha256sum /usr/local/bin/silo 2>/dev/null | cut -d" " -f1' | tr -d '\r'; }
has_program_sha() { [ "$(sha_inside "$1")" = "$2" ]; }
registered_in() { # computer file name
  inside "$1" "grep -c 'silo' $2 2>/dev/null; true" | tr -d '\r' | tail -1; }

cleanup() {
  say "cleanup (drops only the e2e-t5b-* computers this run made, then stops the Dev engine it started)"
  if engine_answers; then
    for c in $F $A; do "$SILO" stop "$c" >/dev/null 2>&1; done
    for round in 1 2 3; do
      for c in $F $A; do "$SILO" status "$c" >/dev/null 2>&1 && "$SILO" drop "$c" --force >/dev/null 2>&1; done
      sleep 2
    done
    say "silo list after cleanup:"; "$SILO" list
  fi
  stop_engine && say "Dev engine stopped" || say "Dev engine did not stop in time"
  rm -rf "$WORK"
}
trap cleanup EXIT

cat >"$WORK/engine.py" <<PY
import json, sys
sys.path.insert(0, "$REPO/docs/evals")
from engine_client import EngineClient
sock, op = sys.argv[1], sys.argv[2]
with EngineClient(sock, client_name="t5b-check") as e:
    if op == "install":
        job = e.call("agents.install", {"computerId": sys.argv[3], "agent": sys.argv[4]})["jobId"]
        print(json.dumps(e.wait_job(job, timeout=900))[:400])
    elif op == "agents":
        print(json.dumps(e.call("agents.list", {"computerId": sys.argv[3]})))
PY

say "== header"
echo "commit $(cd "$REPO" && git rev-parse --short HEAD) on $(cd "$REPO" && git branch --show-current); $("$SILO" --version 2>&1); host $(sw_vers -productVersion) $(uname -m); $(date -u +%FT%TZ)"
WANT=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["binarySha256"])' "$GUEST_MANIFEST" 2>/dev/null)
echo "bundled guest silo: manifest sha256 ${WANT:-?}; file sha256 $(shasum -a 256 "$GUEST_FILE" 2>/dev/null | cut -d' ' -f1); $(file -b "$GUEST_FILE" 2>/dev/null | cut -c1-70)"
if engine_answers && [ -z "$(our_engine)" ]; then
  echo "A Dev engine this script did not start is answering; refusing to touch it." >&2
  trap - EXIT; rm -rf "$WORK"; exit 2
fi
[ -n "$(our_engine)" ] || { start_engine; check "the Dev engine starts from the bundle" $? "pid $(our_engine), log $LOG"; }
echo "computers before: $("$SILO" list 2>/dev/null | awk 'NR>1{print $1}' | tr '\n' ' ')"
for c in $A $F; do "$SILO" status "$c" >/dev/null 2>&1 && { echo "$c already exists; refusing to run" >&2; exit 2; }; done

say "== S1 a new Linux computer gets the program, the receipt and the registration by itself"
"$SILO" create $A --linux --cpus 2 --memory 4 --disk 12 >/dev/null 2>&1; check "create $A" $? ""
"$SILO" start $A >/dev/null 2>&1; check "start $A" $? ""
wait_for 240 has_program_sha $A "$WANT"
check "S1 /usr/local/bin/silo has the bundled file's checksum, nobody passed a binary" $? "$(sha_inside $A)"
echo "ls: $(root $A 'ls -l /usr/local/bin/silo /usr/local/libexec/silo-computer-use' | tr -d '\r' | tr '\n' ';')"
receipt=$(root $A 'cat /var/lib/silo/components.json')
echo "receipt: $receipt"
echo "$receipt" | python3 -c 'import json,sys; v=json.load(sys.stdin); assert v["components"]["silo"]["state"]=="ok" and v["components"]["silo"]["sha256"]=="'"$WANT"'"'
check "S1 the receipt in the computer lists the silo program as ok with that checksum" $? ""
echo "status: $(tools_status $A | tr '\n' '|')"
[ "$(component_state $A silo)" = ok ]; check "S1 the status lists the program as ok" $? ""
seen=$(inside $A 'silo --json list' | python3 -c 'import json,sys; v=json.load(sys.stdin); rows=v if isinstance(v,list) else v.get("computers",[]); print(" ".join(r["name"] for r in rows))')
check "S1 silo list inside works and shows $A" $([ "$seen" = "$A" ]; echo $?) "$seen"

say "== S2 agents.install (Claude Code) registers silo's MCP server beside computer use"
ID_A=$("$SILO" --json status $A | python3 -c 'import json,sys; v=json.load(sys.stdin); print(v.get("id") or v.get("configuration",{}).get("id",""))')
cu_ready() { root $A 'grep -q "\"state\": \"ready\"" /var/lib/silo-computer-use/receipt.json'; }
wait_for 400 cu_ready; check "S2 computer use has finished setting up in $A (so its registration exists to sit beside)" $? ""
echo "agents before: $(engine agents "$ID_A")"
out=$(engine install "$ID_A" claude-code 2>&1); check "S2 agents.install claude-code" $? "$out"
servers_of() { inside "$1" 'python3 -c "import json;print(\" \".join(sorted(json.load(open(\"/home/silo/.claude.json\")).get(\"mcpServers\",{}))))"' | tr -d '\r' | tail -1; }
echo "--- mcpServers in ~/.claude.json of $A:"; inside $A 'python3 -m json.tool /home/silo/.claude.json | sed -n "/mcpServers/,/^    }/p"' | sed 's/^/  /'
echo "$(servers_of $A)" | grep -qw silo; check "S2 the configuration lists the silo MCP server" $? "$(servers_of $A)"
echo "$(servers_of $A)" | grep -qw cua-driver; check "S2 ... next to the computer-use one" $? ""
cp "$REPO/docs/v1/checks/phase5_mcp_client.py" "$WORK/mcp_client.py"
echo '[{"list": true}, {"call": "list", "args": {}}]' >"$WORK/steps.json"
B64_MCP=$(base64 <"$WORK/mcp_client.py" | tr -d '\n'); B64_STEPS=$(base64 <"$WORK/steps.json" | tr -d '\n')
put_mcp() { root "$1" "echo $B64_MCP | base64 -d > /usr/local/bin/t5mcp.py; echo $B64_STEPS | base64 -d > /tmp/t5steps.json; chmod 644 /tmp/t5steps.json" >/dev/null; }
put_mcp $A
CMD=$(inside $A 'python3 -c "import json;c=json.load(open(\"/home/silo/.claude.json\"))[\"mcpServers\"][\"silo\"];print(c[\"command\"],*c[\"args\"])"' | tr -d '\r' | tail -1)
echo "registered command: $CMD"
out=$(inside $A "cd /tmp && python3 /usr/local/bin/t5mcp.py /usr/local/bin/silo /tmp /tmp/t5steps.json")
echo "$out" | cut -c1-500 | sed 's/^/  mcp: /'
mcp_names() { python3 -c '
import json,sys
for line in sys.stdin:
    try: v=json.loads(line)
    except Exception: continue
    if v.get("call")=="list": print(" ".join(sorted(c["name"] for c in json.loads(v["text"])["computers"])))
    elif "tools" in v: print("tools:%d" % len(v["tools"]))'; }
names=$(echo "$out" | mcp_names | tr '\n' ' ')
echo "$names" | grep -q "tools:[1-9]" && echo "$names" | grep -qw -- "$A"; check "S2 an MCP client inside gets a tool list from silo mcp, and a call answers with $A" $? "$names"

say "== S3 a fork has its own endpoint and the registration works against it"
"$SILO" fork $A --name $F >/dev/null 2>&1; check "S3 fork $A" $? ""
"$SILO" start $F >/dev/null 2>&1; check "S3 start the fork" $? ""
ID_A=$("$SILO" --json status $A | python3 -c 'import json,sys; v=json.load(sys.stdin); print(v.get("id") or v.get("configuration",{}).get("id",""))')
ID_F=$("$SILO" --json status $F | python3 -c 'import json,sys; v=json.load(sys.stdin); print(v.get("id") or v.get("configuration",{}).get("id",""))')
fork_ready() { [ "$(root $F 'python3 -c "import json; print(json.load(open(\"/var/lib/silo/components.json\")).get(\"computerId\",\"\"))"' | tr -d '\r')" = "$ID_F" ]; }
wait_for 240 fork_ready; check "S3 the fork's first start wrote its own receipt (computer id $ID_F)" $? ""
fpa=$(root $A 'python3 -c "import json; d=json.load(open(\"/etc/silo/endpoint.json\")); print(d[\"port\"], d[\"computerId\"])"' | tr -d '\r')
fpf=$(root $F 'python3 -c "import json; d=json.load(open(\"/etc/silo/endpoint.json\")); print(d[\"port\"], d[\"computerId\"])"' | tr -d '\r')
echo "endpoint $A: $fpa; endpoint $F: $fpf"
check "S3 the fork's endpoint is its own (another port, its own computer id)" $([ "$fpa" != "$fpf" ] && [ "${fpf#* }" = "$ID_F" ]; echo $?) ""
echo "$(servers_of $F)" | grep -qw silo; check "S3 the fork is registered" $? "$(servers_of $F)"
put_mcp $F
out=$(inside $F "cd /tmp && python3 /usr/local/bin/t5mcp.py /usr/local/bin/silo /tmp /tmp/t5steps.json")
echo "$out" | cut -c1-300 | sed 's/^/  mcp in fork: /'
fnames=$(echo "$out" | mcp_names | tr '\n' ' '); echo "names in the fork: $fnames"
echo "$fnames" | grep -q "tools:[1-9]" && echo "$fnames" | grep -qw -- "$F" && ! echo "$fnames" | grep -qw -- "$A"; check "S3 silo mcp in the fork answers as $F, not as $A" $? "$fnames"

say "== S4 turning the tools off removes the registration, keeps the program, and the program refuses"
out=$("$SILO" tools disable $A 2>&1); check "S4 silo tools disable $A" $? "$out"
sleep 3
echo "$(servers_of $A)" | grep -qw silo; check "S4 the silo MCP server is gone from the running computer's configuration" $([ $? -ne 0 ]; echo $?) "$(servers_of $A)"
echo "$(servers_of $A)" | grep -qw cua-driver; check "S4 computer use's registration is still there" $? ""
check "S4 the program is still in the computer" $([ "$(sha_inside $A)" = "$WANT" ]; echo $?) ""
out=$(inside $A "silo list" 2>&1); check "S4 silo inside now refuses" $([ $? -ne 0 ]; echo $?) "$(echo "$out" | tail -1)"
echo "status: $(tools_status $A | tr '\n' '|')"
"$SILO" stop $F >/dev/null 2>&1
out=$("$SILO" tools disable $F 2>&1); check "S4 silo tools disable $F while it is stopped" $? "$out"
"$SILO" start $F >/dev/null 2>&1
gone() { ! servers_of $F | grep -qw silo; }
wait_for 120 gone; check "S4 the registration is taken out at the stopped computer's next start" $? ""
out=$("$SILO" tools enable $A 2>&1); check "S4 silo tools enable $A again" $? "$(echo "$out" | tr '\n' '|')"
echo "$(servers_of $A)" | grep -qw silo; check "S4 the registration is back" $? "$(servers_of $A)"
"$SILO" tools enable $F >/dev/null 2>&1

say "== S5 a deleted or replaced program comes back at the next start"
root $A 'rm -f /usr/local/bin/silo'
root $A 'test -e /usr/local/bin/silo'; check "S5 the program is deleted" $([ $? -ne 0 ]; echo $?) ""
restart $A
wait_for 240 has_program_sha $A "$WANT"; check "S5 the right program is back after a restart" $? "$(sha_inside $A)"
root $A 'printf "#!/bin/sh\necho an agent was here\n" > /usr/local/bin/silo; chmod 755 /usr/local/bin/silo'
check "S5 the program is replaced by an agent" $([ "$(sha_inside $A)" != "$WANT" ]; echo $?) "$(sha_inside $A | cut -c1-12)"
restart $A
wait_for 240 has_program_sha $A "$WANT"; check "S5 the right program is back after a restart" $? ""

say "== S6 a reconcile that fails leaves the computer running and shows the failure"
root $A 'printf "x" > /usr/local/bin/silo && chattr +i /usr/local/bin/silo' ; echo "chattr: rc=$?"
restart $A
failed() { [ "$(component_state $A silo)" = failed ]; }
wait_for 240 failed; check "S6 the status shows the program as failed" $? ""
echo "status: $(tools_status $A | tr '\n' '|')"
out=$(inside $A 'echo still-running'); check "S6 the computer is running and answers" $([ "$out" = "still-running" ]; echo $?) "$out"
[ "$("$SILO" --json status $A | python3 -c 'import json,sys; v=json.load(sys.stdin); print(v.get("state") or v.get("detail") or "")')" != "" ]; echo "computer status: $("$SILO" status $A 2>&1 | head -2 | tr '\n' '|')"
root $A 'chattr -i /usr/local/bin/silo'
restart $A
wait_for 240 has_program_sha $A "$WANT"; check "S6 after the fix the next start recovers the program" $? ""
[ "$(component_state $A silo)" = ok ]; check "S6 and the status is ok again" $? ""

say "== S7 --binary goes through the engine's path and the next start puts the shipped program back"
cp "$GUEST_FILE" "$WORK/silo-dev"; printf '\0' >>"$WORK/silo-dev"
DEV=$(shasum -a 256 "$WORK/silo-dev" | cut -d' ' -f1)
out=$("$SILO" tools enable $A --binary "$WORK/silo-dev" 2>&1); check "S7 silo tools enable --binary" $? "$(echo "$out" | tr '\n' '|')"
check "S7 the computer has the program that was named (checked inside by root)" $([ "$(sha_inside $A)" = "$DEV" ]; echo $?) "$(sha_inside $A | cut -c1-12)"
restart $A
wait_for 240 has_program_sha $A "$WANT"; check "S7 the next start puts the shipped program back" $? ""

say "== S8 macOS computers on the Dev engine"
mac=$("$SILO" list 2>/dev/null | awk 'NR>1 && /macos/ {print $1}' | tr '\n' ' ')
if [ -n "$mac" ]; then echo "macOS computers present: $mac (not driven by this script)"; else echo "no macOS computer exists on the Dev engine; the macOS push over SSH and the macOS registration are not verified live"; fi

say "== engine log lines about the channels and tools"
grep -E "channel of|tools of|components of|endpoint file" "$LOG" | tail -25 | sed 's/^/  /'
say "== summary: $PASS passed, $FAILN failed"
