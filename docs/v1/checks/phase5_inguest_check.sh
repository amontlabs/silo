#!/usr/bin/env bash
# Silo v1 phase 5 VM check: the in-guest client against the Dev engine (Linux computer). Run:
#   bash docs/v1/checks/phase5_inguest_check.sh 2>&1 | tee docs/v1/evidence/phase-5/inguest-check.txt
# Only `silo` (incl. `silo exec` in e2e-p5fix-* computers) plus read-only host commands.
# Steps follow docs/SiloUI-IN-GUEST-CLIENT.md and docs/v1/BUILD_LOG.md (5a).
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO="$REPO/app/SiloUI/target/debug/silo"
GUESTBIN="$REPO/app/SiloUI/target/aarch64-unknown-linux-musl/release/silo"
BUNDLE="$REPO/app/SiloUI/target/debug/bundle/macos/Silo Dev.app"
ENGINE_LOG="$REPO/app/SiloUI/src-tauri/target/verification/silod-dev-p5fix.log"
RUNTIME="$HOME/Library/Application Support/org.silo.dev/runtime/microsandbox"
SILO_OK=0; HOSTMADE=0
MAIN=e2e-p5fix-main; IN1=e2e-p5fix-in1; IN2=e2e-p5fix-in2; HOST1=e2e-p5fix-host1
ARCH=""
PASS=0; FAILN=0; BLOCKED=0
TMPD=$(mktemp -d "${TMPDIR:-/tmp}/e2e-p5.XXXXXX")
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
blocked() { echo "[$(ts)] BLOCKED  $1  -- $2"; BLOCKED=$((BLOCKED+1)); }
names() { "$SILO" list 2>/dev/null | awk 'NR>1{print $1}' | sort; }
xr() { local c=$1; shift; "$SILO" exec "$c" --timeout 170 -- sudo bash -c "$*"; }
xu() { local c=$1; shift; "$SILO" exec "$c" --timeout 170 -- sudo -u silo bash -c "$*"; }
BEFORE=$(names)
cleanup() {
  say "cleanup (drops only computers this run created)"
  local made c
  made=$(comm -13 <(echo "$BEFORE") <(names) | grep -E '^e2e-p5fix-' ; [ -n "$ARCH" ] && echo "$ARCH")
  made=$(echo "$made" | sort -u | grep -v '^$')
  for c in $made; do "$SILO" stop "$c" >/dev/null 2>&1; done
  for round in 1 2 3 4; do
    for c in $made; do "$SILO" status "$c" >/dev/null 2>&1 && "$SILO" drop "$c" --force >/dev/null 2>&1; done
    sleep 3
  done
  rm -rf "$TMPD"; say "silo list after cleanup:"; "$SILO" list
}
trap cleanup EXIT

FRAME=$(cat <<'PY'
import json,sys,os,uuid,time
tok=json.load(open(os.environ.get('F','/etc/silo/guest-client.json')))
method=sys.argv[1]; params=json.loads(sys.argv[2]) if len(sys.argv)>2 else {}
spool=tok.get('spool') or '/.msb/silo-bridge'
i=uuid.uuid4().hex
fr={'v':1,'id':i,'token':tok['token'],'method':method,'params':params}
tmp='%s/req/%s.json.tmp'%(spool,i)
fd=os.open(tmp,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600); os.write(fd,json.dumps(fr).encode()); os.close(fd)
os.rename(tmp,'%s/req/%s.json'%(spool,i))
end=time.time()+float(os.environ.get('W','20'))
while time.time()<end:
    p='%s/resp/%s.json'%(spool,i)
    try:
        print(open(p).read()); sys.exit(0)
    except (IOError,OSError):
        pass
    time.sleep(0.2)
print('NO_ANSWER id='+i); sys.exit(3)
PY
)
FRAME_B64=$(printf '%s' "$FRAME" | base64 | tr -d '\n')

echo "== header"
echo "engine/CLI commit: $(git -C "$REPO" rev-parse HEAD); branch $(git -C "$REPO" branch --show-current); tracked changes under app/: [$(git -C "$REPO" status --porcelain app | tr '\n' ' ')]"
echo "bundled msb: $("$BUNDLE/Contents/MacOS/msb" --version 2>&1 | head -1)"
echo "host macOS: $(sw_vers -productVersion) ($(uname -m)); silo: $("$SILO" --version); date $(date -u +%FT%TZ)"
echo "guest binary: $(file "$GUESTBIN" | cut -c1-150); sha256 $(shasum -a 256 "$GUESTBIN" | cut -c1-16)"
echo "computers before: $(echo $BEFORE)"

echo "== create, start, install"
"$SILO" create $MAIN --linux --cpus 2 --memory 2 --disk 8 >/dev/null 2>&1 && "$SILO" start $MAIN >/dev/null 2>&1; check "create+start $MAIN" $? ""
echo "guest: $(xr $MAIN '. /etc/os-release; echo $PRETTY_NAME; uname -m' | tr '\n' ' ')"
out=$("$SILO" guest-client install $MAIN --agent openclaw --binary "$GUESTBIN" 2>&1); rc=$?; echo "$out"; check "guest-client install --binary (upload of the guest binary)" $rc ""
if [ $rc -ne 0 ]; then
  echo "-- token file after the failed install: $(xr $MAIN 'ls -l /etc/silo/guest-client.json 2>&1')"
  echo "-- guest-client status after the failed install: $("$SILO" guest-client status $MAIN 2>&1 | tr '\n' ' ')"
fi
out=$(xr $MAIN 'sha256sum /usr/local/bin/silo | cut -c1-16'); echo "guest binary sha256 prefix: $out (host: $(shasum -a 256 "$GUESTBIN" | cut -c1-16))"
[ "$out" = "$(shasum -a 256 "$GUESTBIN" | cut -c1-16)" ]; check "installed binary matches the uploaded one" $? ""
echo "-- install with a missing file leaves no token (atomic):"
xr $MAIN 'rm -f /etc/silo/guest-client.json'; "$SILO" guest-client revoke $MAIN >/dev/null 2>&1
out=$("$SILO" guest-client install $MAIN --agent openclaw --binary "$TMPD/does-not-exist" 2>&1); rc2=$?; echo "$out"
[ $rc2 -ne 0 ]; check "install --binary with a missing file fails" $? ""
xr $MAIN 'test ! -e /etc/silo/guest-client.json'; check "no token file after the failed install" $? ""
"$SILO" guest-client status $MAIN 2>&1 | head -3
out=$("$SILO" guest-client install $MAIN --agent openclaw --binary "$GUESTBIN" 2>&1); rc=$?; echo "$out"; check "guest-client install --binary (second, real install)" $rc ""
"$SILO" guest-client status $MAIN
xr $MAIN "echo $FRAME_B64 | base64 -d > /usr/local/bin/p5frame.py; command -v python3; ls -l /usr/local/bin/silo /etc/silo/guest-client.json; /usr/local/bin/silo --version"
MAIN_ID=$("$SILO" status $MAIN --json | python3 -c "import json,sys;print(json.load(sys.stdin)['id'])")
echo "host bridge folder: $(ls -ld "$RUNTIME/sandboxes/$MAIN/runtime/silo-bridge" "$RUNTIME/sandboxes/$MAIN/runtime/silo-bridge/req" "$RUNTIME/sandboxes/$MAIN/runtime/silo-bridge/resp" 2>&1 | tr '\n' ';')"

as_account() { # label, runner (xr|xu)
  local who=$1 run=$2
  echo "== inside the guest as $who"
  out=$($run $MAIN 'ls -ld /.msb /.msb/silo-bridge /.msb/silo-bridge/req /.msb/silo-bridge/resp; id -un; ls -l /etc/silo/guest-client.json; test -r /etc/silo/guest-client.json && echo token-readable; test -x /.msb/silo-bridge && test -w /.msb/silo-bridge/req && test -x /.msb/silo-bridge/resp && echo folders-reachable' 2>&1); echo "$out"
  echo "$out" | grep -q 'token-readable' && echo "$out" | grep -q 'folders-reachable'; check "[$who] bridge folders reachable (search silo-bridge, write req, enter resp), token file readable" $? ""
  out=$($run $MAIN 'silo --in-guest list' 2>&1); rc=$?; echo "$out"
  [ $rc = 0 ] && echo "$out" | grep -q "$MAIN"; check "[$who] silo --in-guest list shows $MAIN" $? "rc=$rc"
  [ $rc = 0 ] && [ $who = silo ] && SILO_OK=1
  echo "$out" | grep -qE "workspace-1|e2e-relay|silo-eval"; [ $? -ne 0 ]; check "[$who] list shows no unrelated computer" $? ""
}
as_account root xr
as_account silo xu

echo "== fork from inside (root), change, diff, request"
out=$(xr $MAIN "silo --in-guest fork $MAIN --name $IN1" 2>&1); rc=$?; echo "$out"
check "[root] in-guest fork $IN1" $rc ""
"$SILO" list | grep "$IN1"
out=$(xr $MAIN 'silo --in-guest list' 2>&1); echo "$out"; echo "$out" | grep -q "$IN1"; check "[root] list now shows $IN1" $? ""
"$SILO" exec $IN1 --timeout 60 -- sudo touch /root/p5-in-guest; check "change in fork ($IN1: /root/p5-in-guest)" $? ""
out=$(xr $MAIN "silo --in-guest diff $IN1" 2>&1); rc=$?; echo "$out" | head -30; echo "$out" | grep -q p5-in-guest; check "[root] in-guest diff shows the change" $? "rc=$rc"
out=$(xr $MAIN "silo --in-guest fold $IN1 --request" 2>&1); rc=$?; echo "$out"; check "[root] fold --request" $rc ""
out=$("$SILO" requests 2>&1); echo "$out"; echo "$out" | grep -q "$IN1"; check "host 'silo requests' shows the request for $IN1" $? ""
REQ_ID=$(echo "$out" | awk -v f="$IN1" '$0 ~ f {print $1; exit}')

echo "== fork from inside as the non-root silo user"
if [ $SILO_OK = 1 ]; then
out=$(xu $MAIN "silo --in-guest fork $MAIN --name $IN2" 2>&1); rc=$?; echo "$out"; check "[silo] in-guest fork $IN2" $rc ""
out=$(xu $MAIN "silo --in-guest diff $IN2; silo --in-guest status $MAIN" 2>&1); rc=$?; echo "$out" | head -20; check "[silo] in-guest diff/status of own fork" $rc ""
out=$(xu $MAIN "silo --in-guest fold $IN2 --request" 2>&1); rc=$?; echo "$out"; check "[silo] fold --request" $rc ""
else echo "skipped: the silo user cannot reach the bridge (FAIL above); the remaining silo-user checks stop here"; fi

echo "== refusals from inside"
refuse() { # label, runner, command
  local out rc; out=$($2 $MAIN "$3" 2>&1); rc=$?; echo "[$1] $3 -> rc=$rc: $out"
  [ $rc -ne 0 ] && echo "$out" | grep -qE 'not available from inside a computer|permission_denied'; check "[$1] refused: $3" $? ""
}
for run in xr xu; do
  who=root; [ $run = xu ] && who=silo
  [ $run = xu ] && [ $SILO_OK != 1 ] && continue
  refuse $who $run "silo --in-guest fold $IN1"
  refuse $who $run "silo --in-guest fold $IN1 --promote"
  refuse $who $run "silo --in-guest undo $MAIN"
  refuse $who $run "silo --in-guest drop $IN1"
  refuse $who $run "silo --in-guest secrets list"
  refuse $who $run "silo --in-guest requests keep ${REQ_ID:-0}"
done
echo "-- hand-written frames"
for run in xr xu; do
  who=root; [ $run = xu ] && who=silo
  [ $run = xu ] && [ $SILO_OK != 1 ] && continue
  for m in changes.keep guest.exec; do
    case $m in changes.keep) p="{\"requestId\":\"${REQ_ID:-0}\"}";; guest.exec) p="{\"computerId\":\"$MAIN_ID\",\"command\":[\"id\"]}";; esac
    out=$($run $MAIN "python3 /usr/local/bin/p5frame.py $m '$p'" 2>&1); echo "[$who] $m -> $out"
    echo "$out" | grep -q permission_denied; check "[$who] frame $m gets permission_denied" $? ""
  done
done

echo "== ownership: a fork made on the host"
out=$("$SILO" fork $MAIN --name $HOST1 2>&1); rc=$?; echo "$out"
check "host fork of running $MAIN after bridge calls (checkpoint of a running computer)" $rc ""
"$SILO" start $HOST1 >/dev/null 2>&1; HOST1_ID=$("$SILO" status $HOST1 --json | python3 -c "import json,sys;print(json.load(sys.stdin)['id'])")
echo "$HOST1 id $HOST1_ID"
out=$(xr $HOST1 'ls -l /etc/silo/ 2>&1; test ! -e /etc/silo/guest-client.json && echo token-file-absent' 2>&1); echo "$out"
echo "$out" | grep -q token-file-absent; check "token file gone in the host-made fork $HOST1" $? ""
out=$(xr $MAIN 'silo --in-guest list' 2>&1); echo "$out" | grep -q "$HOST1"; [ $? -ne 0 ]; check "[root] list does not show the host's fork" $? ""
out=$(xr $MAIN "silo --in-guest diff $HOST1" 2>&1); rc=$?; echo "diff $HOST1 -> rc=$rc: $out"; [ $rc -ne 0 ]; check "diff of host fork refused/not found" $? ""
for m in computers.stop computers.start changes.diff; do
  out=$(xr $MAIN "python3 /usr/local/bin/p5frame.py $m '{\"computerId\":\"$HOST1_ID\"}'" 2>&1); echo "frame $m $HOST1 -> $out"
  echo "$out" | grep -q '"ok":false'; check "frame $m on host fork refused" $? ""
done
"$SILO" status $HOST1 | head -3

echo "== symlink attempt on resp/"
ls -1 /tmp | sort > "$TMPD/tmp-before"
RUNTIME_BEFORE=$(find "$RUNTIME/sandboxes/$MAIN/runtime" -maxdepth 3 2>/dev/null | sort | md5)
LOGN=$(wc -l < "$ENGINE_LOG" 2>/dev/null || echo 0)
out=$(xr $MAIN 'mv /.msb/silo-bridge/resp /.msb/silo-bridge/resp.real && ln -s /tmp /.msb/silo-bridge/resp && ls -ld /.msb/silo-bridge/resp* && W=10 python3 /usr/local/bin/p5frame.py computers.tree' 2>&1); echo "$out"
SYM_ID=$(echo "$out" | sed -n 's/.*NO_ANSWER id=//p')
echo "$out" | grep -q 'resp -> /tmp'; check "symlink replaced resp inside the guest" $? ""
echo "$out" | grep -q '"ok":true'; [ $? -ne 0 ]; check "no answer delivered through the symlink" $? ""
sleep 5
ls -1 /tmp | sort > "$TMPD/tmp-after"
echo "host /tmp new entries: [$(diff "$TMPD/tmp-before" "$TMPD/tmp-after" | grep '^>' | tr '\n' ' ')]"
[ -z "$SYM_ID" ] || { test ! -e "/tmp/$SYM_ID.json"; check "host /tmp has no <request id>.json (symlink not followed)" $? "id=$SYM_ID"; }
diff "$TMPD/tmp-before" "$TMPD/tmp-after" | grep '^>' | grep -qE '[0-9a-f]{32}\.json'; [ $? -ne 0 ]; check "no answer-shaped file appeared in host /tmp" $? ""
echo "engine log delta:"; tail -n +$((LOGN+1)) "$ENGINE_LOG" 2>/dev/null | cut -c1-300 | head -20
xr $MAIN 'rm /.msb/silo-bridge/resp && mv /.msb/silo-bridge/resp.real /.msb/silo-bridge/resp && ls -ld /.msb/silo-bridge/resp'
ls -l "$RUNTIME/sandboxes/$MAIN/runtime/silo-bridge/"
sleep 3
out=$(xr $MAIN 'silo --in-guest list' 2>&1); rc=$?; echo "after restoring resp: rc=$rc"; echo "$out" | head -5
check "bridge serves again after the folder is restored (informational)" $rc ""

echo "== the fork's token"
out=$(xr $IN1 'ls -l /etc/silo/ 2>&1; test ! -e /etc/silo/guest-client.json && echo token-file-absent' 2>&1); echo "$out"
echo "$out" | grep -q token-file-absent; check "token file gone in $IN1" $? ""
CP=$(xr $MAIN 'base64 -w0 /etc/silo/guest-client.json')
out=$("$SILO" guest-client install $IN1 --agent openclaw 2>&1); rc=$?; echo "$out"; check "install the client into $IN1 (gives it a bridge folder and its own token)" $rc ""
xr $IN1 'ls -ld /.msb/silo-bridge'
out=$(xr $IN1 'silo --in-guest list' 2>&1); rc=$?; echo "$IN1 with its own token: rc=$rc: $out"
[ $rc = 0 ] && echo "$out" | grep -q "$IN1"; check "$IN1's own token works there" $? ""
xr $IN1 "echo $CP | base64 -d > /etc/silo/guest-client.json && chgrp silo /etc/silo/guest-client.json && chmod 0640 /etc/silo/guest-client.json && ls -l /etc/silo/guest-client.json"
out=$(xr $IN1 'W=20 silo --in-guest list' 2>&1); rc=$?; echo "parent's token copied into $IN1: rc=$rc: $out"
[ $rc -ne 0 ] && ! echo "$out" | grep -q "$MAIN"; check "copied parent token refused in the fork (which has a bridge folder)" $? "rc=$rc"
xr $IN1 'rm -f /etc/silo/guest-client.json'

echo "== promote on the host: the old main's token ends"
N0=$(names); "$SILO" list
out=$("$SILO" fold $IN1 --promote 2>&1); rc=$?; echo "$out"; check "silo fold $IN1 --promote (computers running)" $rc ""
N1=$(names); "$SILO" list
ARCH=$(comm -13 <(echo "$N0") <(echo "$N1") | head -1); echo "archived old main is named: [$ARCH]"
"$SILO" guest-client status $MAIN 2>&1 | head -5   # the name now belongs to the promoted fork
if [ -n "$ARCH" ]; then
  "$SILO" start "$ARCH" >/dev/null 2>&1; sleep 5
  echo "old main status: $("$SILO" guest-client status "$ARCH" 2>&1 | head -3 | tr '\n' ' ')"
  out=$(xr "$ARCH" 'silo --in-guest list' 2>&1); rc=$?; echo "old main: rc=$rc: $out"
  [ $rc -ne 0 ]; check "old main's token is revoked (silo --in-guest list fails)" $? "rc=$rc"
else blocked "old main token revoked" "no archived computer name found after promote"; fi

echo "== undo and revoke"
out=$("$SILO" undo $MAIN 2>&1); rc=$?; echo "$out"; check "silo undo $MAIN" $rc ""
"$SILO" list
"$SILO" status $MAIN --json | python3 -c "import json,sys;d=json.load(sys.stdin);print('main back, id matches original:', d['id']=='$MAIN_ID')"
out=$("$SILO" guest-client revoke $MAIN 2>&1); rc=$?; echo "$out"; check "guest-client revoke" $rc ""
"$SILO" guest-client status $MAIN

echo "== summary: PASS=$PASS FAIL=$FAILN BLOCKED=$BLOCKED"
