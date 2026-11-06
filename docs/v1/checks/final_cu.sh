# Section 2 of final_run.sh (sourced): computer use in a fork (screenshot, type into a terminal, verify a file),
# then promote that auto fork over the ask main, and undo.
CLIENT="$CHECKS/phase5_mcp_client.py"; POINTER="$CHECKS/phase5_pointer.py"
mcp() { local cwd=$1 out=$2 steps; steps=$(mktemp "${TMPDIR:-/tmp}/final-steps.XXXXXX"); export SILO RAW POINTER
  python3 -c "import json,os,sys
S=os.environ['SILO']; R=os.environ['RAW']; P=os.environ['POINTER']
json.dump(eval(sys.stdin.read()), open('$steps','w'))"; python3 "$CLIENT" "$SILO" "$cwd" "$steps" > "$out" 2>&1; rm -f "$steps"; }

cu_linux() {
cd "$PROJ" || return
F=e2e-final-gui
echo "== C1 stub agent binaries on main (so the guest helper registers an agent), then fork --gui --computer-use auto"
xs main 'install -d ~/.local/bin; for a in claude codex; do printf "#!/bin/sh\nexit 0\n" > ~/.local/bin/$a; chmod +x ~/.local/bin/$a; done'
say "main desktop settle 60 s"; sleep 60
echo "main: $("$SILO" status main | grep -i 'computer use')"
drop_one $F  # left by an earlier run
t0=$(now); "$SILO" fork main --name $F --gui --computer-use auto --json > "$RAW/cu-fork.json" 2> "$RAW/cu-fork.err"; rc=$?; secs=$(elapsed "$t0")
tail -3 "$RAW/cu-fork.err"; cu=$(jfield computerUse.state < "$RAW/cu-fork.json"); ap=$(jfield computerUse.approval < "$RAW/cu-fork.json")
[ $rc = 0 ] && [ "$cu" = ready ] && [ "$ap" = auto ]; check "C1 fork --gui auto is ready" $? "rc=$rc ${secs}s state=$cu approval=$ap"
echo "== C2 MCP: screenshot, open a terminal, type, verify the file from the host"
mcp "$PROJ" "$RAW/cu-mcp.jsonl" <<PY
[{"call":"computer_use_ready","args":{"computer":"$F"}},
 {"call":"computer_use_screenshot","args":{"computer":"$F"},"save":R+"/final-cu-1-desktop"},
 {"call":"computer_use_hotkey","args":{"computer":"$F","keys":["ctrl","alt","t"]}},{"sleep":4},
 {"call":"computer_use_screenshot","args":{"computer":"$F"},"save":R+"/final-cu-2-terminal"},
 {"call":"computer_use_click","args":{"computer":"$F","x":300,"y":250}},
 {"call":"computer_use_type","args":{"computer":"$F","text":"echo final-cu-ok > /tmp/finalcu.txt"}},
 {"call":"computer_use_key","args":{"computer":"$F","key":"Return"}},{"sleep":2},
 {"call":"computer_use_screenshot","args":{"computer":"$F"},"save":R+"/final-cu-3-after-typing"}]
PY
python3 -c "
import json
for l in open('$RAW/cu-mcp.jsonl'):
    if not l.startswith('{'): continue
    r=json.loads(l); print('  step',r['step'],r.get('call') or '','isError=',r.get('isError'),[(i['size'],i['bytes']) for i in r.get('images',[])],(r.get('text') or '').replace(chr(10),' | ')[:160])"
grep -c '"isError": false' "$RAW/cu-mcp.jsonl" | sed 's/^/tool calls without error: /'
out=$(xs $F 'cat /tmp/finalcu.txt' | head -1); echo "silo exec $F -- cat /tmp/finalcu.txt -> $out"
[ "$out" = final-cu-ok ]; check "C2 typing into the fork's GUI terminal wrote a file the host reads" $? "$out"
ls "$RAW"/final-cu-*.png 2>/dev/null | sed 's/^/  screenshot: /'
}

promote_linux() {
cd "$PROJ" || return
F=e2e-final-gui
echo "== P1 promote the auto fork $F over main (approval ask)"
echo "main before: $("$SILO" status main | grep -i 'computer use')"; MID=$("$SILO" status main --json | jfield id)
xs $F 'echo promoted-final > ~/final-marker'
"$SILO" fold $F --promote --json > "$RAW/promote.json" 2> "$RAW/promote.err"; rc=$?; tail -3 "$RAW/promote.err"; check "P1 silo fold --promote exits 0" $rc ""
arch=$("$SILO" list | awk '/^main-before-/{print $1}' | head -1); echo "archive: $arch"
[ "$("$SILO" status main --json | jfield id)" != "$MID" ] && [ "$("$SILO" status "$arch" --json | jfield id)" = "$MID" ]; check "P1 the fork is now main; old main archived" $? "$arch"
for i in $(seq 40); do [ "$(state main)" = running ] && break; sleep 3; done
echo "marker on new main: $(xs main 'cat ~/final-marker' | head -1)"
echo "main after: $("$SILO" status main | grep -i 'computer use')"
"$SILO" status main | grep -i 'computer use' | grep -q 'approval ask'; check "P1 promoted main keeps computer-use approval ask (not the fork's auto)" $? ""
echo "== P2 silo undo main"
"$SILO" undo main > "$RAW/promote-undo.txt" 2>&1; rc=$?; tail -4 "$RAW/promote-undo.txt"; check "P2 silo undo main exits 0" $rc ""
[ "$("$SILO" status main --json | jfield id)" = "$MID" ] && [ -z "$("$SILO" list | grep main-before-)" ]; check "P2 original main is back, no archive left" $? ""
for i in $(seq 40); do [ "$(state main)" = running ] && break; sleep 3; done
xs main 'test ! -e ~/final-marker'; check "P2 main has no promoted marker" $? ""
drop_one $F
}
