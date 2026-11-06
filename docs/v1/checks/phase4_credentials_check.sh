#!/usr/bin/env bash
# Silo v1 phase 4 VM check: the "credentials used" line of a fork, Linux then macOS. Run:
#   bash docs/v1/checks/phase4_credentials_check.sh 2>&1 | tee docs/v1/evidence/phase-4/credentials-check.txt
# Only `silo` (incl. `silo exec` in e2e-p4-* computers) plus read-only host commands.
#
# Destinations are public HTTPS echo services, because a computer cannot reach the Mac: egress
# denies loopback and private ranges, so host.microsandbox.internal refuses every port
# (docs/THREAT-MODEL.md). The secret is a fake value allowed for ALLOWED only; the proxy intercepts
# TLS there and substitutes it in headers (Silo assigns secrets with header substitution only).
# OTHER is not allowed: Silo assigns secrets with `passthrough=*` (engine/src/secrets_runtime.rs),
# so the placeholder goes there unchanged and nothing is recorded as blocked. The echoed X-P4
# header is compared by sha256 on the host; the value itself is never printed.
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO=${SILO:-"$REPO/app/SiloUI/target/debug/silo"}
ALLOWED=postman-echo.com; ALLOWED_URL=https://postman-echo.com/post
OTHER=httpbin.org; OTHER_URL=https://httpbin.org/anything
DATA_HOME="$HOME/Library/Application Support/org.silo.dev"
FAKE=p4-fake-not-real
FAKE_SHA=$(printf %s "$FAKE" | shasum -a 256 | cut -d' ' -f1)
PH_SHA=$(printf %s '$MSB_E2E_P4_TOKEN' | shasum -a 256 | cut -d' ' -f1)
PASS=0; FAILN=0; BLOCKED=0
CREATED=()
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
blocked() { echo "[$(ts)] BLOCKED  $1  -- $2"; BLOCKED=$((BLOCKED+1)); }
cleanup() {
  say "cleanup"
  for c in "${CREATED[@]:-}"; do [ -n "$c" ] && "$SILO" stop "$c" >/dev/null 2>&1; done
  for c in "${CREATED[@]:-}"; do [ -n "$c" ] || continue; for i in $(seq 30); do "$SILO" drop "$c" --force >/dev/null 2>&1 && break; "$SILO" status "$c" >/dev/null 2>&1 || break; sleep 3; done; done
  say "silo list after cleanup:"; "$SILO" list
  echo "SUMMARY: PASS=$PASS FAIL=$FAILN BLOCKED=$BLOCKED"
}
trap cleanup EXIT

echo "== header"
echo "commit: $(git -C "$REPO" rev-parse HEAD); silo: $("$SILO" --version); macOS $(sw_vers -productVersion) $(uname -m); date $(date -u +%FT%TZ)"
echo "allowed: $ALLOWED; not allowed: $OTHER; fake value sha256 ${FAKE_SHA:0:12}..."

# POST to URL from computer C with X-P4: $E2E_P4_TOKEN; prints the sha256 of the echoed header (or "curl-failed: ...").
echo_sha() {
  local out
  out=$("$SILO" exec "$1" --timeout 60 -- bash -c "curl -sS -m 20 -H \"X-P4: \$E2E_P4_TOKEN\" -d p4=1 $2 2>&1") || { echo "curl-failed: $(echo "$out" | tail -1 | cut -c1-120)"; return; }
  printf %s "$out" | python3 -c '
import json, sys, hashlib
raw = sys.stdin.read()
try:
    h = {k.lower(): v for k, v in json.loads(raw[raw.index("{"):raw.rindex("}") + 1])["headers"].items()}
    print(hashlib.sha256(h.get("x-p4", "").encode()).hexdigest())
except Exception as e:
    print("unparsed:", type(e).__name__)'
}

run_kind() { # kind(linux|macos)
  local kind=$1 flag SRC=e2e-p4-src FORK=e2e-p4-fork CTL=e2e-p4-ctl
  [ "$kind" = macos ] && { SRC=e2e-p4-msrc; FORK=e2e-p4-mfork; CTL=e2e-p4-mctl; }
  echo "== $kind"
  if [ "$kind" = macos ]; then
    local vms; vms=$(pgrep -f Virtualization.VirtualMachine | wc -l | tr -d ' ')
    if [ "$vms" -ge 1 ]; then blocked "macOS credentials check" "$vms macOS VMs already running (this check runs 2 at once; limit 2)"; return; fi
    flag=(--macos --cpus 4 --memory 8)
  else
    flag=(--linux --cpus 2 --memory 2 --disk 8)
  fi
  CREATED+=("$SRC" "$FORK" "$CTL")
  "$SILO" create $SRC "${flag[@]}" >/dev/null 2>&1; check "$kind: create $SRC" $? ""
  local out; out=$(printf %s "$FAKE" | "$SILO" secrets set E2E_P4_TOKEN --computer $SRC --domain $ALLOWED 2>&1); local rc=$?
  if [ $rc -ne 0 ]; then
    if echo "$out" | grep -q credential_access_denied; then blocked "$kind: secrets set + all later steps" "credential_access_denied: $out"; else check "$kind: secrets set" 1 "$out"; fi
    return
  fi
  check "$kind: secrets set" 0 ""
  # The source stays stopped: forks do not need it, and two macOS VMs is the limit.
  "$SILO" fork $SRC --name $FORK --secret E2E_P4_TOKEN >/dev/null 2>&1; check "$kind: fork with --secret" $? ""
  "$SILO" fork $SRC --name $CTL >/dev/null 2>&1; check "$kind: control fork without --secret" $? ""
  for f in $FORK $CTL; do out=$("$SILO" start $f 2>&1); check "$kind: start $f" $? "$(echo "$out" | tail -1)"; done
  local ph; ph=$("$SILO" exec $FORK --timeout 60 -- bash -c 'printf %s "${E2E_P4_TOKEN:-}"' 2>/dev/null)
  [ "$ph" = '$MSB_E2E_P4_TOKEN' ]; check "$kind: fork sees the placeholder, not the value" $? "${ph:0:9}..."
  ph=$("$SILO" exec $CTL --timeout 60 -- bash -c 'printf %s "${E2E_P4_TOKEN:-}"' 2>/dev/null)
  [ -z "$ph" ]; check "$kind: control fork has no E2E_P4_TOKEN" $? ""

  local got r n=0
  for i in 1 2 3; do r=$(echo_sha $FORK $ALLOWED_URL); got="$got ${r:0:12}"; [ "$r" = "$FAKE_SHA" ] && n=$((n+1)); done
  [ $n = 3 ]; check "$kind: $ALLOWED received the real value (sha256) for 3 fork POSTs" $? "matches=$n/3 sha:$got"
  r=$(echo_sha $FORK $OTHER_URL)
  [ "$r" = "$PH_SHA" ]; check "$kind: $OTHER (not allowed) received the placeholder unchanged" $? "sha ${r:0:12} (placeholder ${PH_SHA:0:12})"
  r=$(echo_sha $CTL $ALLOWED_URL)
  [ "$r" != "$FAKE_SHA" ]; check "$kind: control fork's POST was not substituted" $? "sha ${r:0:12}"

  sleep 3
  local want="credentials used: E2E_P4_TOKEN → $ALLOWED (POST ×3)"
  local st; st=$("$SILO" status $FORK 2>&1); echo "$st" | grep -i 'credentials used'
  echo "$st" | grep -qF "$want" && ! echo "$st" | grep -q 'blocked:'
  check "$kind: status shows used ×3 and nothing else" $? ""
  "$SILO" status $FORK --json | python3 -c "
import json,sys
d=json.load(sys.stdin); c=d.get('credentialsUsed') or {}
print(json.dumps(c))
u=c.get('used',[]); b=c.get('blocked',[])
sys.exit(0 if len(u)==1 and u[0]['secret']=='E2E_P4_TOKEN' and u[0]['host']=='$ALLOWED' and u[0]['method']=='POST' and u[0]['count']==3 and not b else 1)"; check "$kind: status --json credentialsUsed counts" $? ""
  local df; df=$("$SILO" diff $FORK 2>&1); echo "$df" | grep -i 'credentials used'
  echo "$df" | grep -qi "credentials used: E2E_P4_TOKEN → $ALLOWED (POST ×3)"; check "$kind: diff shows the line" $? ""
  local rq; rq=$("$SILO" fold $FORK --request 2>&1); echo "$rq" | tail -2
  local id; id=$(echo "$rq" | grep -o 'fr-[0-9a-f]\{8\}' | head -1)
  local shown; shown=$("$SILO" requests show "$id" 2>&1); echo "$shown" | grep -i 'credentials used'
  # `requests show` writes the ASCII form (cli/src/render.rs credentials_lines): "to" and "x3".
  echo "$shown" | grep -qF "Credentials used: E2E_P4_TOKEN to $ALLOWED (POST x3)"; check "$kind: requests show $id has the same credentials" $? ""
  local cs; cs=$("$SILO" status $CTL 2>&1); echo "$cs" | grep -i 'credentials used'
  echo "$cs" | grep -qi 'credentials used: none'; check "$kind: control fork shows credentials used: none" $? ""
  say "usage files on the host for $FORK: must not hold the value or the placeholder"
  # Linux: runtime/microsandbox/sandboxes/<name>/logs/; macOS: macos-computers/<id>/.
  local fid; fid=$("$SILO" status $FORK --json | python3 -c 'import json,sys; print(json.load(sys.stdin).get("id") or "-")')
  local files; files=$(find "$DATA_HOME" -name 'secret-usage*' -not -name '*.lock' \( -path "*/$FORK/*" -o -path "*/$fid/*" \) 2>/dev/null)
  echo "usage files:"; echo "$files" | sed "s|$HOME|~|"
  if [ -z "$files" ]; then check "$kind: usage files found" 1 "none under $DATA_HOME"; else
    local f bad=0; while IFS= read -r f; do grep -q -e "$FAKE" -e 'MSB_E2E_P4_TOKEN' "$f" && bad=1; done <<< "$files"
    check "$kind: usage files hold neither the value nor the placeholder" $bad ""
  fi
  for f in $FORK $CTL; do "$SILO" stop $f >/dev/null 2>&1; done
}

run_kind linux
run_kind macos
