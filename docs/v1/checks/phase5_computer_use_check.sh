#!/usr/bin/env bash
# Silo v1 phase 5 VM check: computer use from the host against the Dev engine. Run:
#   bash docs/v1/checks/phase5_computer_use_check.sh [linux|macos|all] 2>&1 | tee docs/v1/evidence/phase-5/computer-use-check.txt
# Only `silo` (incl. `silo exec` in e2e-p5cu-* computers), `silo mcp` driven by phase5_mcp_client.py, plus
# read-only host commands. Computers: e2e-p5cu-* only; the cleanup trap drops only the ones listed here.
# Never more than 2 macOS VMs at once.
set -uo pipefail
MODE=${1:-all}
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO="$REPO/app/SiloUI/target/debug/silo"
BUNDLE="$REPO/app/SiloUI/target/debug/bundle/macos/Silo Dev.app"
CLIENT="$REPO/docs/v1/checks/phase5_mcp_client.py"
POINTER="$REPO/docs/v1/checks/phase5_pointer.py"
FIXTURE="$REPO/app/SiloUI/cli/tests/fixtures/cua-driver-0.38.0-tools.json"
RAW="$REPO/docs/v1/evidence/phase-5/raw"
SRC=e2e-p5cu-src; F1=e2e-p5cu-f1; NOGUI=e2e-p5cu-nogui; NOGUI2=e2e-p5cu-nogui2
MSRC=e2e-p5cu-msrc; MF1=e2e-p5cu-mf1
WORK=$(mktemp -d "${TMPDIR:-/tmp}/e2e-p5cu.XXXXXX")
PLAIN="$WORK/plain"; AUTOP="$WORK/auto"
mkdir -p "$PLAIN/.silo" "$AUTOP/.silo" "$RAW"
printf 'computer_use_auto: true\n' > "$AUTOP/.silo/silo.yaml"
PASS=0; FAILN=0; BLOCKED=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { if [ "$2" = 0 ]; then echo "[$(ts)] PASS  $1  -- $3"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $1  -- $3"; FAILN=$((FAILN+1)); fi; }
blocked() { echo "[$(ts)] BLOCKED  $1  -- $2"; BLOCKED=$((BLOCKED+1)); }
nvms() { "$SILO" list | awk '$2=="running" && $3=="macos"' | wc -l | tr -d ' '; }
state() { "$SILO" status "$1" --json 2>/dev/null | python3 -c "import json,sys;print(json.load(sys.stdin)['state'])" 2>/dev/null; }
jget() { python3 -c "import json,sys;d=json.load(sys.stdin)
for k in sys.argv[1].split('.'): d=d[k]
print(d)" "$1" 2>/dev/null; }
x() { local c=$1; shift; "$SILO" exec "$c" --timeout 120 -- "$@" 2>&1 | grep -v '^step [0-9]*$'; return "${PIPESTATUS[0]}"; }
# runs steps (a python list expression using S for the silo path) in one `silo mcp` session; result lines go to $2
mcp() { local cwd=$1 out=$2; python3 -c "import json,os,sys
S=os.environ['SILO']; R=os.environ['RAW']; P=os.environ['POINTER']
json.dump(eval(sys.stdin.read()), open('$WORK/steps.json','w'))"; python3 "$CLIENT" "$SILO" "$cwd" "$WORK/steps.json" > "$out" 2>&1; }
cleanup() {
  say "cleanup (only e2e-p5cu-* computers)"
  for c in $MF1 $F1 $NOGUI $NOGUI2; do "$SILO" stop "$c" >/dev/null 2>&1; done
  for c in $MF1 $F1 $NOGUI $NOGUI2 $MSRC $SRC; do "$SILO" stop "$c" >/dev/null 2>&1; done
  for c in $MF1 $F1 $NOGUI $NOGUI2 $MSRC $SRC; do for i in $(seq 40); do "$SILO" status "$c" >/dev/null 2>&1 || break; "$SILO" drop "$c" --force >/dev/null 2>&1 && break; sleep 3; done; done
  rm -rf "$WORK"; say "silo list after cleanup:"; "$SILO" list; "$SILO" list | grep -c e2e-p5cu | sed 's/^/e2e-p5cu computers left: /'
  echo "macOS VMs running: $(nvms)"; echo "== summary ($MODE): $PASS passed, $FAILN failed, $BLOCKED blocked"
}
trap cleanup EXIT
export SILO RAW POINTER

echo "== header"
echo "commit: $(git -C "$REPO" rev-parse HEAD); git status --porcelain app: [$(git -C "$REPO" status --porcelain app | tr '\n' ' ')]"
echo "bundled msb: $("$BUNDLE/Contents/MacOS/msb" --version 2>&1 | head -1); silo $("$SILO" --version); host macOS $(sw_vers -productVersion) ($(uname -m)); date $(date -u +%FT%TZ)"
echo "macOS VMs running at start (silo list; pgrep Virtualization.VirtualMachine lines: $(pgrep -f Virtualization.VirtualMachine | wc -l | tr -d " ")): $(nvms); mode: $MODE"; "$SILO" list

linux() {
echo "== LINUX: source $SRC (stub claude/codex binaries so the guest helper registers them)"
[ -z "$(state $SRC)" ] && { "$SILO" create $SRC --linux --cpus 2 --memory 4 --disk 10 >/dev/null 2>&1; check "create source" $? ""; }
[ "$(state $SRC)" = running ] || { "$SILO" start $SRC >/dev/null 2>&1; check "start source" $? ""; }
x $SRC bash -c 'install -d ~/.local/bin; for a in claude codex; do printf "#!/bin/sh\nexit 0\n" > ~/.local/bin/$a; chmod +x ~/.local/bin/$a; done; ls ~/.local/bin'
say "letting the source desktop settle 60 s (a panel config written after the fork makes the keep clash)"; sleep 60
echo "source status: $("$SILO" status $SRC | grep 'computer use')"

echo "== L1 silo fork --gui --computer-use auto (CLI, person)"
t0=$(date +%s); (cd "$PLAIN" && "$SILO" fork $SRC --name $F1 --gui --computer-use auto --json 2>"$WORK/fork.err" > "$WORK/fork.json"); rc=$?; secs=$(( $(date +%s) - t0 ))
tail -3 "$WORK/fork.err"; python3 -c "import json;print(json.dumps(json.load(open('$WORK/fork.json')).get('computerUse')))"
cu=$(jget computerUse.state < "$WORK/fork.json"); ap=$(jget computerUse.approval < "$WORK/fork.json")
[ $rc = 0 ] && [ "$cu" = ready ] && [ "$ap" = auto ]; check "L1 fork --gui auto ready" $? "rc=$rc ${secs}s state=$cu approval=$ap"
echo "fork status: $("$SILO" status $F1 | grep 'computer use')"
echo "fork agent files:"; x $F1 bash -c 'grep -n cua-driver ~/.claude/settings.json; grep -n -i "cua-driver\|approve" ~/.codex/config.toml'
x $F1 bash -c 'grep -q mcp__cua-driver ~/.claude/settings.json && grep -q approve ~/.codex/config.toml'; check "L1 the auto fork has the cua-driver allow rule and Codex approve line" $? ""

echo "== L2 MCP session on $F1"
mcp "$PLAIN" "$WORK/m1.jsonl" <<PY
[{"list":True},
 {"call":"computer_use_tools","args":{"computer":"$F1"}},
 {"call":"computer_use_ready","args":{"computer":"$F1"}},
 {"call":"computer_use_screenshot","args":{"computer":"$F1"},"save":R+"/linux-1-desktop"},
 {"call":"computer_use_click","args":{"computer":"$F1","x":640,"y":400}},
 {"sh":S+" exec $F1 -- env DISPLAY=:1 XAUTHORITY=/run/silo-desktop/user/Xauthority python3 -c \"\$(cat "+P+")\""},
 {"call":"computer_use_hotkey","args":{"computer":"$F1","keys":["ctrl","alt","t"]}},
 {"sleep":4},
 {"call":"computer_use_screenshot","args":{"computer":"$F1"},"save":R+"/linux-2-terminal"},
 {"call":"computer_use_click","args":{"computer":"$F1","x":300,"y":250}},
 {"call":"computer_use_type","args":{"computer":"$F1","text":"echo p5-cu-ok > /tmp/p5cu.txt"}},
 {"call":"computer_use_key","args":{"computer":"$F1","key":"Return"}},
 {"sleep":2},
 {"call":"computer_use_hotkey","args":{"computer":"$F1","keys":["ctrl","l"]}},
 {"call":"computer_use_scroll","args":{"computer":"$F1","x":640,"y":400,"direction":"down","amount":3}},
 {"call":"computer_use_screenshot","args":{"computer":"$F1"},"save":R+"/linux-3-after-typing"}]
PY
python3 - "$WORK/m1.jsonl" "$FIXTURE" <<'PY'
import json, sys
rows = [json.loads(l) for l in open(sys.argv[1]) if l.startswith("{")]
for r in rows:
    t = (r.get("text") or "").replace("\n", " | ")[:260]
    print(f'  step {r["step"]} {r.get("call") or r.get("sh","")[:40] or r.get("tools") or ""} isError={r.get("isError")} images={[(i["size"], i["bytes"]) for i in r.get("images", [])]} {t}')
fx = sorted(json.load(open(sys.argv[2]))["tools"])
by = {r["step"]: r for r in rows}
tools = by[1]["text"]
try:
    j = json.loads(tools)
    names = sorted(t["name"] if isinstance(t, dict) else t for t in (j["tools"] if isinstance(j, dict) else j))
except Exception:
    names = sorted(n for n in fx if n in tools)
open(sys.argv[1] + ".names", "w").write("%s\n%s\n" % (json.dumps(names), json.dumps(fx)))
PY
{ read -r got; read -r want; } < "$WORK/m1.jsonl.names"; echo "driver tools: $got"
[ "$got" = "$want" ]; check "L2 computer_use_tools matches the pinned fixture" $? "fixture $want"
python3 - "$WORK/m1.jsonl" <<'PY'
import json, sys
rows = {json.loads(l)["step"]: json.loads(l) for l in open(sys.argv[1]) if l.startswith("{")}
imgs = [i for r in rows.values() for i in r.get("images", [])]
w = [i["size"][0] for i in imgs if i["size"]]
ok = bool(w) and max(w) <= 1280 and "1280x800" in rows[3]["text"] and "screen 1440x900" in rows[3]["text"]
sys.exit(0 if ok else 1)
PY
check "L2 screenshots are images no wider than 1280, text gives size and the screen size" $? "$(grep -o 'Screenshot 1280x800 (the screen is [0-9x]*)' "$WORK/m1.jsonl" | head -1); files in raw/linux-*"
# step 5 is the pointer read after clicking screenshot pixel (640,400); with the screen 1440x900 it must be (720,450)
ptr=$(grep '"step": 5' "$WORK/m1.jsonl" | grep -o 'pointer [0-9]* [0-9]*'); echo "pointer after click at screenshot pixel (640,400) on a 1440x900 screen: $ptr"
[ "$ptr" = "pointer 720 450" ]; check "L2 a click read off the (1280-wide) screenshot lands scaled on the 1440x900 screen" $? "$ptr (expected 720 450)"
grep -q 'Terminal' "$WORK/m1.jsonl"; check "L2 ctrl+alt+t opens a terminal (window list shows Terminal)" $? ""
out=$(x $F1 cat /tmp/p5cu.txt); echo "silo exec $F1 -- cat /tmp/p5cu.txt -> $out"
[ "$out" = p5-cu-ok ]; check "L2 typing into the fork's GUI terminal wrote a file the host reads with silo exec" $? "$out"
grep -q '"call": "computer_use_hotkey".*isError": false' "$WORK/m1.jsonl" && grep -q '"call": "computer_use_scroll".*isError": false' "$WORK/m1.jsonl"; check "L2 hotkey ctrl+l and scroll answer without error" $? ""

echo "== L2b first click in a fresh silo mcp session (no screenshot first) lands at the mapped position"
mcp "$PLAIN" "$WORK/m1b.jsonl" <<PY
[{"call":"computer_use_click","args":{"computer":"$F1","x":320,"y":200}},
 {"sh":S+" exec $F1 -- env DISPLAY=:1 XAUTHORITY=/run/silo-desktop/user/Xauthority python3 -c \"\$(cat "+P+")\""}]
PY
python3 -c "
import json
for l in open('$WORK/m1b.jsonl'):
    r=json.loads(l); print('  step',r['step'],r.get('call') or 'pointer','isError/rc=',r.get('isError',r.get('rc')),(r.get('text') or r.get('out') or '').replace(chr(10),' | ')[:200])"
ptr=$(grep '"step": 1' "$WORK/m1b.jsonl" | grep -o 'pointer [0-9]* [0-9]*'); echo "pointer after first-ever click at pixel (320,200) with no prior screenshot: $ptr"
[ "$ptr" = "pointer 360 225" ]; check "L2b first click with no screenshot first lands at the mapped position" $? "$ptr (expected 360 225 on a 1440x900 screen)"

echo "== L3 kill the driver in the fork, next call works (same silo mcp session as step before)"
mcp "$PLAIN" "$WORK/m2.jsonl" <<PY
[{"call":"computer_use_screenshot","args":{"computer":"$F1"}},
 {"sh":S+" exec $F1 -- bash -c 'pgrep -af \"cua-driver mcp\" | grep -v perl | cut -c1-100' 2>&1 | grep -v '^step'"},
 {"sh":S+" exec $F1 -- pkill -f 'cua-driver mcp' 2>&1; true"},
 {"sleep":3},
 {"sh":S+" exec $F1 -- bash -c 'pgrep -af \"cua-driver mcp\" | grep -v perl | grep -v pgrep | wc -l' 2>&1 | grep -v '^step'"},
 {"call":"computer_use_screenshot","args":{"computer":"$F1"},"save":R+"/linux-4-after-driver-kill"},
 {"call":"computer_use_click","args":{"computer":"$F1","x":300,"y":250}},
 {"call":"computer_use_ready","args":{"computer":"$F1"}}]
PY
python3 -c "
import json
for l in open('$WORK/m2.jsonl'):
    r=json.loads(l); print('  step',r['step'],r.get('call') or r.get('sh','')[:60],'rc/isError=',r.get('rc',r.get('isError')),(r.get('out') or r.get('text') or '').replace(chr(10),' | ')[:200])"
python3 -c "
import json
rows={json.loads(l)['step']:json.loads(l) for l in open('$WORK/m2.jsonl')}
ok=rows[0]['isError'] is False and rows[5]['isError'] is False and rows[6]['isError'] is False
ok=ok and 'cua-driver mcp' in rows[1]['out'] and rows[4]['out'].strip()=='0'
raise SystemExit(0 if ok else 1)"
check "L3 driver killed (pgrep before/after), the next tool calls start a new driver and work" $? "see steps"

echo "== L4 silo diff $F1 after the session"
"$SILO" diff $F1 2>&1 | grep -v '^step [0-9]*$'
"$SILO" diff $F1 --json 2>/dev/null | python3 -c "
import json,sys
d=json.load(sys.stdin)
def walk(o,p=''):
    if isinstance(o,dict):
        for k,v in o.items(): walk(v,p+'/'+k)
    elif isinstance(o,list):
        for v in o: print('  json',p,json.dumps(v)[:160])
walk(d)" | head -40
"$SILO" diff $F1 --json 2>/dev/null | grep -qE 'silo-cu|cua-driver|silo-computer-use|streamer'; [ $? -ne 0 ]; check "L4 no driver, helper or screenshot file in the change list" $? "recorded above; /tmp/p5cu.txt is under the ignored /tmp"

echo "== L5 refusals"
mcp "$PLAIN" "$WORK/m3.jsonl" <<PY
[{"call":"computer_use_screenshot","args":{"computer":"$SRC"}},
 {"call":"computer_use_tools","args":{"computer":"$SRC"}},
 {"call":"computer_use_click","args":{"computer":"$SRC","x":10,"y":10}},
 {"call":"fork_gui","args":{"source":"$SRC","name":"$NOGUI","computer_use":"auto"}}]
PY
python3 -c "
import json
rows=[json.loads(l) for l in open('$WORK/m3.jsonl')]
for r in rows: print('  step',r['step'],r['call'],'isError=',r['isError'],r['text'][:200])
raise SystemExit(0 if all(r['isError'] for r in rows[:3]) and 'not a fork' in rows[0]['text'] else 1)"
check "L5 host computer_use_* tools refuse the source (not a fork)" $? ""
python3 -c "
import json
rows=[json.loads(l) for l in open('$WORK/m3.jsonl')]
raise SystemExit(0 if rows[3]['isError'] and 'computer_use_auto' in rows[3]['text'] else 1)"
check "L5 fork_gui computer_use auto refused without computer_use_auto in .silo/silo.yaml" $? "no $NOGUI created: [$("$SILO" list | grep $NOGUI)]"
out=$(cd "$PLAIN" && SILO_AGENT=claude "$SILO" fork $SRC --name $NOGUI2 --gui --computer-use auto 2>&1 | tail -2); echo "$out"
echo "$out" | grep -q computer_use_auto; check "L5 CLI with SILO_AGENT set and no computer_use_auto refuses --computer-use auto" $? "[$("$SILO" list | grep $NOGUI2)]"
echo "source approval: $("$SILO" status $SRC | grep 'computer use')"
echo "$("$SILO" status $SRC | grep 'computer use')" | grep -q 'approval ask'; check "L5 source status still says approval ask" $? ""
out=$(cd "$PLAIN" && "$SILO" fork $SRC --name $NOGUI2 --computer-use auto --json 2>&1 | head -3); "$SILO" status $SRC | grep -q 'approval ask'; check "L5 fork without --gui is fine; source still ask" $? ""
"$SILO" drop $NOGUI2 --force >/dev/null 2>&1

echo "== L6 keep $F1 (approval auto) into $SRC (ask)"
"$SILO" fold $F1 2>&1 | grep -v '^step [0-9]*$' | tail -8; frc=${PIPESTATUS[0]}; echo "fold rc=$frc"; [ $frc = 0 ]; check "L6 silo fold $F1 keeps into $SRC" $? "rc=$frc"
say "waiting 20 s for the guest update after the keep"; sleep 20
echo "source after keep: $("$SILO" status $SRC | grep 'computer use')"
echo "source files after keep (read as root: the keep may leave them root-owned):"
x $SRC sudo bash -c 'ls -la /home/silo/.claude /home/silo/.codex 2>&1; echo "--- settings.json:"; cat /home/silo/.claude/settings.json 2>&1; echo "--- config.toml:"; cat /home/silo/.codex/config.toml 2>&1; echo "--- silo-computer-use.log (tail):"; tail -15 /var/log/silo-computer-use.log 2>&1'
x $SRC sudo bash -c '! grep -q "mcp__cua-driver" /home/silo/.claude/settings.json 2>/dev/null && ! grep -q "approve" /home/silo/.codex/config.toml 2>/dev/null'; check "L6 source has no mcp__cua-driver auto rule and no approve line after the keep" $? ""
x $SRC sudo bash -c 'for f in /home/silo/.claude/settings.json /home/silo/.codex/config.toml; do [ ! -e $f ] || [ "$(stat -c %U $f)" = silo ] || exit 1; done'; check "L6 the kept settings files are owned by the working account (silo)" $? "owners: $(x $SRC sudo stat -c '%U:%a %n' /home/silo/.claude/settings.json /home/silo/.codex/config.toml | tr '\n' ' ')"
"$SILO" status $SRC | grep 'computer use' | grep -q 'failed'; [ $? -ne 0 ]; check "L6 source computer use is not 'failed' after the keep" $? "$("$SILO" status $SRC | grep 'computer use')"
"$SILO" status $SRC | grep -q 'approval ask'; check "L6 source status says approval ask after the keep" $? ""
}

macos() {
  echo "== MACOS"
  [ "$(nvms)" -lt 2 ] || { blocked "macOS section" "2 macOS VMs already running"; return; }
  if [ -z "$(state $MSRC)" ]; then
    t0=$(date +%s); "$SILO" create $MSRC --macos --cpus 4 --memory 8 --disk 40 2>&1 | grep -v '^Creating .*: [0-9]*s$'; rc=${PIPESTATUS[0]}
    echo "create rc=$rc in $(( $(date +%s) - t0 )) s"
    [ $rc = 0 ] || { blocked "macOS section" "silo create --macos failed rc=$rc"; return; }
  fi
  [ "$(state $MSRC)" = running ] || "$SILO" start $MSRC 2>&1 | grep -v '^Starting .*: [0-9]*s$'
  echo "macOS source: $(x $MSRC bash -c 'sw_vers | tr "\n" " "; id -un')"

  echo "== M1 silo fork $MSRC --gui --computer-use auto (the wait must reach ready)"
  [ "$(nvms)" -lt 2 ] || { blocked "macOS fork" "2 macOS VMs already running"; return; }
  t0=$(date +%s); (cd "$PLAIN" && "$SILO" fork $MSRC --name $MF1 --gui --computer-use auto --gui-timeout 120 --json 2>"$WORK/mfork.err" > "$WORK/mfork.json"); rc=$?; secs=$(( $(date +%s) - t0 ))
  tail -2 "$WORK/mfork.err"; cu=$(jget computerUse.state < "$WORK/mfork.json")
  [ $rc = 0 ] && [ "$cu" = ready ]; check "M1 macOS fork --gui returns with computerUse.state ready" $? "rc=$rc ${secs}s state=[$cu]; engine status: $("$SILO" status $MF1 | grep 'computer use'); ready tool below"
  "$SILO" status $MF1 | grep -q 'approval auto'; check "M1 the macOS fork's recorded approval is auto" $? "$("$SILO" status $MF1 | grep 'computer use')"
  [ "$(state $MF1)" = running ] || { blocked "M2-M4" "$MF1 not running"; return; }
  echo "== M3 MCP: screenshot, spotlight Terminal, click, type, Return"
  mcp "$PLAIN" "$WORK/mm1.jsonl" <<PY
[{"call":"computer_use_ready","args":{"computer":"$MF1"}},
 {"call":"computer_use_screenshot","args":{"computer":"$MF1","max_side":900},"save":R+"/macos-1-desktop"},
 {"call":"computer_use_hotkey","args":{"computer":"$MF1","keys":["cmd","space"]}},{"sleep":2},
 {"call":"computer_use_type","args":{"computer":"$MF1","text":"Terminal"}},{"sleep":2},
 {"call":"computer_use_key","args":{"computer":"$MF1","key":"Return"}},{"sleep":4},
 {"call":"computer_use_screenshot","args":{"computer":"$MF1","max_side":900},"save":R+"/macos-2-terminal"},
 {"call":"computer_use_click","args":{"computer":"$MF1","x":450,"y":250}},
 {"call":"computer_use_type","args":{"computer":"$MF1","text":"echo p5-cu-ok > /tmp/p5cu.txt"}},
 {"call":"computer_use_key","args":{"computer":"$MF1","key":"Return"}},{"sleep":2},
 {"call":"computer_use_screenshot","args":{"computer":"$MF1","max_side":900},"save":R+"/macos-3-after-typing"}]
PY
  python3 -c "
import json
for l in open('$WORK/mm1.jsonl'):
    r=json.loads(l); print('  step',r['step'],r.get('call') or '', 'isError=',r.get('isError'),[(i['size'],i['bytes']) for i in r.get('images',[])],(r.get('text') or '').replace(chr(10),' | ')[:200])"
  python3 -c "
import json
rows={json.loads(l)['step']:json.loads(l) for l in open('$WORK/mm1.jsonl')}
ok=rows[1]['isError'] is False and rows[1]['images'] and rows[1]['images'][0]['size'][0]<=900
raise SystemExit(0 if ok else 1)"
  check "M3 computer_use_screenshot returns an image (<= max_side)" $? ""
  out=$(x $MF1 cat /tmp/p5cu.txt); echo "silo exec $MF1 -- cat /tmp/p5cu.txt -> $out"
  [ "$out" = p5-cu-ok ]; check "M3 click + type + Return in the fork's GUI Terminal wrote the file the host reads" $? "$out"

  echo "== M2 cua-driver permissions status in the fork (after M3: no LaunchAgent starts the daemon, the first cua-driver mcp session does)"
  out=""; for _ in 1 2 3 4 5 6; do out=$(x $MF1 /usr/local/bin/cua-driver permissions status --json); echo "$out" | grep -q '"daemon_running": *false' || break; sleep 5; done; echo "$out"
  echo "$out" | python3 -c "
import json,sys
t=sys.stdin.read(); d=json.loads(t[t.index('{'):])
s=json.dumps(d).lower()
raise SystemExit(0 if d.get('status')!='unknown' and 'accessibility' in s and 'screen' in s else 1)"
  check "M2 permissions status lists Accessibility and Screen Recording as granted" $? "see output above"
  echo "receipt in fork: $(x $MF1 bash -c 'cat "$HOME/Library/Application Support/Silo/computer-use-receipt.json"')"


  echo "== M4 keep the auto fork into its ask source"
  x $MF1 bash -c 'echo "fork claude: $(command -v claude)"; grep -n cua-driver ~/.claude/settings.json 2>&1; grep -n -i "cua-driver\|approve" ~/.codex/config.toml 2>&1'
  x $MF1 bash -c 'grep -q mcp__cua-driver ~/.claude/settings.json && grep -q approve ~/.codex/config.toml'; check "M4 before the keep the fork has the auto rule and approve line (so the keep tests something)" $? ""
  "$SILO" fold $MF1 2>&1 | grep -v '^step [0-9]*$' | tail -6; frc=${PIPESTATUS[0]}; echo "fold rc=$frc"; [ $frc = 0 ]; check "M4 silo fold $MF1 keeps into $MSRC" $? "rc=$frc"
  say "waiting 60 s for the guest update after the keep"; sleep 60
  echo "source after keep: $("$SILO" status $MSRC | grep 'computer use')"
  x $MSRC bash -c 'echo "--- settings.json:"; cat ~/.claude/settings.json 2>&1; echo "--- config.toml:"; cat ~/.codex/config.toml 2>&1; echo "--- p5cu.txt: $(cat /tmp/p5cu.txt 2>&1)"'
  x $MSRC bash -c '! grep -q "mcp__cua-driver" ~/.claude/settings.json 2>/dev/null && ! grep -q "approve" ~/.codex/config.toml 2>/dev/null'; check "M4 source has no mcp__cua-driver rule and no approve line after the keep" $? ""
  "$SILO" status $MSRC | grep -q 'approval ask'; check "M4 source status says approval ask after the keep" $? ""
  blocked "M4 promote an auto fork over an ask main" "needs a computer named main; this check never creates or touches main"
}
case $MODE in linux) linux;; macos) macos;; all) linux; macos;; esac
