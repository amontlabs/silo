#!/usr/bin/env bash
# Silo v1 phase 4 VM check: the "credentials used" line of a fork, Linux then macOS. Run:
#   bash docs/v1/checks/phase4_credentials_check.sh 2>&1 | tee docs/v1/evidence/phase-4/credentials-check.txt
# Only `silo` (incl. `silo exec` in e2e-p4-* computers) plus read-only host commands and a loopback HTTPS
# listener (python3) started here. The guest reaches the host's loopback at host.microsandbox.internal; the
# secret is allowed for that host, so the proxy intercepts TLS for it (require_tls_identity) and substitutes.
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO="$REPO/app/SiloUI/target/debug/silo"
HOSTNAME_GUEST=host.microsandbox.internal
PORT=${P4_PORT:-18443}
FAKE=p4-fake-not-real
FAKE_SHA=$(printf %s "$FAKE" | shasum -a 256 | cut -d' ' -f1)
WORK=$(mktemp -d "${TMPDIR:-/tmp}/e2e-p4.XXXXXX")
PASS=0; FAILN=0; BLOCKED=0
CREATED=()
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
blocked() { echo "[$(ts)] BLOCKED  $1  -- $2"; BLOCKED=$((BLOCKED+1)); }
x() { local c=$1; shift; "$SILO" exec "$c" --timeout 120 -- sudo bash -c "$*"; }
LISTENER=
cleanup() {
  say "cleanup"
  [ -n "$LISTENER" ] && kill "$LISTENER" 2>/dev/null
  for c in "${CREATED[@]:-}"; do [ -n "$c" ] && "$SILO" stop "$c" >/dev/null 2>&1; done
  for c in "${CREATED[@]:-}"; do [ -n "$c" ] || continue; for i in $(seq 30); do "$SILO" drop "$c" --force >/dev/null 2>&1 && break; "$SILO" status "$c" >/dev/null 2>&1 || break; sleep 3; done; done
  rm -rf "$WORK"; say "silo list after cleanup:"; "$SILO" list
  echo "SUMMARY: PASS=$PASS FAIL=$FAILN BLOCKED=$BLOCKED"
}
trap cleanup EXIT

echo "== header"
echo "commit: $(git -C "$REPO" rev-parse HEAD); silo: $("$SILO" --version); macOS $(sw_vers -productVersion) $(uname -m); date $(date -u +%FT%TZ)"

# Listener: HTTPS on loopback, logs method, path and sha256 of the X-P4 header and body (never the values).
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$WORK/k.pem" -out "$WORK/c.pem" -days 1 -subj "/CN=$HOSTNAME_GUEST" >/dev/null 2>&1
cat > "$WORK/srv.py" <<'PY'
import http.server, ssl, hashlib, sys, json
log = sys.argv[3]
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        n = int(self.headers.get("Content-Length") or 0); body = self.rfile.read(n)
        h = (self.headers.get("X-P4") or "").encode()
        open(log, "a").write(json.dumps({"m": "POST", "hdr": hashlib.sha256(h).hexdigest(), "body": hashlib.sha256(body).hexdigest()}) + "\n")
        self.send_response(200); self.end_headers(); self.wfile.write(b"ok")
    def log_message(self, *a): pass
s = http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H)
c = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); c.load_cert_chain(sys.argv[2] + "/c.pem", sys.argv[2] + "/k.pem")
s.socket = c.wrap_socket(s.socket, server_side=True); s.serve_forever()
PY
: > "$WORK/hits.jsonl"
python3 "$WORK/srv.py" "$PORT" "$WORK" "$WORK/hits.jsonl" & LISTENER=$!
sleep 1

run_kind() { # kind(linux|macos)
  local kind=$1 flag SRC=e2e-p4-src FORK=e2e-p4-fork CTL=e2e-p4-ctl
  [ "$kind" = macos ] && { SRC=e2e-p4-msrc; FORK=e2e-p4-mfork; CTL=e2e-p4-mctl; }
  echo "== $kind"
  if [ "$kind" = macos ]; then
    local vms; vms=$(pgrep -f Virtualization.VirtualMachine | wc -l | tr -d ' ')
    if [ "$vms" -ge 2 ]; then blocked "macOS credentials check" "$vms macOS VMs already running (limit 2)"; return; fi
    flag=(--macos --cpus 4 --memory 8)
  else
    flag=(--linux --cpus 2 --memory 2 --disk 8)
  fi
  CREATED+=("$SRC" "$FORK" "$CTL")
  "$SILO" create $SRC "${flag[@]}" >/dev/null 2>&1 && "$SILO" start $SRC >/dev/null 2>&1; check "$kind: create+start $SRC" $? ""
  local out; out=$(printf %s "$FAKE" | "$SILO" secrets set E2E_P4_TOKEN --computer $SRC --domain $HOSTNAME_GUEST 2>&1); local rc=$?
  if [ $rc -ne 0 ]; then
    if echo "$out" | grep -q credential_access_denied; then blocked "$kind: secrets set + all later steps" "credential_access_denied: $out"; else check "$kind: secrets set" 1 "$out"; fi
    return
  fi
  check "$kind: secrets set" 0 ""
  x $SRC 'command -v curl >/dev/null || (apt-get update -qq && apt-get install -y -qq curl) >/dev/null 2>&1; echo placeholder-in-guest: $(printenv E2E_P4_TOKEN | sed "s/^\(.\{9\}\).*/\1.../")'
  "$SILO" fork $SRC --name $FORK --secret E2E_P4_TOKEN >/dev/null 2>&1; check "$kind: fork with --secret" $? ""
  "$SILO" fork $SRC --name $CTL >/dev/null 2>&1; check "$kind: control fork without --secret" $? ""
  for f in $FORK $CTL; do "$SILO" start $f >/dev/null 2>&1; check "$kind: start $f" $? ""; done
  : > "$WORK/hits.jsonl"
  for f in $FORK $CTL; do
    for i in 1 2 3; do x $f "curl -sk -m 20 -X POST -H \"X-P4: \$E2E_P4_TOKEN\" -d \"\$E2E_P4_TOKEN\" https://$HOSTNAME_GUEST:$PORT/p$i >/dev/null"; done
    x $f 'curl -sk -m 10 -H "X-P4: $E2E_P4_TOKEN" https://example.invalid/ >/dev/null 2>&1; true'
    if [ $f = $FORK ]; then FORK_HITS=$(wc -l < "$WORK/hits.jsonl" | tr -d ' '); fi
  done
  local total; total=$(wc -l < "$WORK/hits.jsonl" | tr -d ' ')
  local fh; fh=$(python3 -c "
import json,sys
L=[json.loads(l) for l in open('$WORK/hits.jsonl')][:$FORK_HITS]
print(sum(1 for r in L if r['hdr']=='$FAKE_SHA' and r['body']=='$FAKE_SHA'), len(L))" 2>&1)
  [ "$fh" = "3 3" ]; check "$kind: listener got the real value (sha256 match, header+body) for 3 fork POSTs" $? "matches/total=$fh"
  local ch; ch=$(python3 -c "
import json
L=[json.loads(l) for l in open('$WORK/hits.jsonl')][$FORK_HITS:]
print(sum(1 for r in L if r['hdr']=='$FAKE_SHA' or r['body']=='$FAKE_SHA'), len(L))")
  case "$ch" in "0 "*) check "$kind: control fork's requests were not substituted" 0 "substituted/total=$ch";; *) check "$kind: control fork's requests were not substituted" 1 "substituted/total=$ch";; esac
  sleep 3
  local st; st=$("$SILO" status $FORK 2>&1); echo "$st" | grep -i 'credentials used'
  echo "$st" | grep -q 'credentials used: E2E_P4_TOKEN → '"$HOSTNAME_GUEST"' (POST ×3); blocked: 1'; check "$kind: status shows used ×3 + blocked 1" $? ""
  "$SILO" status $FORK --json | python3 -c "
import json,sys
d=json.load(sys.stdin); c=d.get('credentialsUsed') or {}
print(json.dumps(c))
u=c.get('used',[]); b=c.get('blocked',[])
sys.exit(0 if any(r['secret']=='E2E_P4_TOKEN' and r['host']=='$HOSTNAME_GUEST' and r['method']=='POST' and r['count']==3 for r in u) and sum(r['count'] for r in b)==1 else 1)"; check "$kind: status --json credentialsUsed counts" $? ""
  local df; df=$("$SILO" diff $FORK 2>&1); echo "$df" | grep -i 'credentials used'
  echo "$df" | grep -q 'credentials used: E2E_P4_TOKEN'; check "$kind: diff shows the line" $? ""
  local rq; rq=$("$SILO" fold $FORK --request 2>&1); echo "$rq" | tail -2
  local id; id=$(echo "$rq" | grep -o 'fr-[0-9a-f]\{8\}' | head -1)
  local shown; shown=$("$SILO" requests show "$id" 2>&1); echo "$shown" | grep -i 'credentials used'
  echo "$shown" | grep -q 'credentials used: E2E_P4_TOKEN → '"$HOSTNAME_GUEST"' (POST ×3); blocked: 1'; check "$kind: requests show $id has the same credentials" $? ""
  local cs; cs=$("$SILO" status $CTL 2>&1); echo "$cs" | grep -i 'credentials used'
  echo "$cs" | grep -q 'credentials used: none'; check "$kind: control fork shows credentials used: none" $? ""
  say "usage files on the host for $FORK / $CTL: must not hold the value or the placeholder"
  local files; files=$(find "$HOME/.silo-dev" -name 'secret-usage*' \( -path "*$FORK*" -o -path "*$CTL*" -o -path "*$SRC*" \) 2>/dev/null)
  echo "usage files: $(echo "$files" | wc -l | tr -d ' ')"
  if [ -z "$files" ]; then check "$kind: usage files found" 1 "none under ~/.silo-dev"; else
    ! grep -l -e "$FAKE" -e 'MSB_E2E_P4_TOKEN' $files >/dev/null 2>&1; check "$kind: usage files hold neither the value nor the placeholder" $? ""
  fi
}

run_kind linux
run_kind macos
