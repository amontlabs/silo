#!/usr/bin/env bash
# T14 live check: an export removes known login files, and where the removed bytes still are.
# Runs against the Dev engine (~/.silo-dev) with throwaway e2e-t14-* computers. Run:
#   bash docs/direction/checks/t14_export_scrub_check.sh 2>&1 | tee docs/direction/evidence/T14.txt
# Needs the Dev `silo` (SILO, default: the T14 target's debug build) and a Dev engine that is running.
#
# What it does, in order:
#   1. A Linux computer gets a fake login at each known login path, all holding one long random
#      marker (M1), plus a key file in a project folder (found by its extension, not by a fixed path). A second marker (M2) is
#      written to both disks and the files are deleted again.
#   2. `silo export` (the default) removes the logins. The CANARY lines then read the whole file the way
#      an import reads it (header, manifest, each payload zstd-decompressed, every disk layer) and look
#      for both markers. They PASS only if neither is in the file; a FAIL names the disk layers that
#      still hold the bytes. That is the acceptance test for claiming the bytes are out of the file.
#   3. `silo export --include-logins` has M1 in it (the positive control: the scan can see the markers).
#   4. The default export imports, the imported computer starts and its login files are gone.
#   5. The source keeps its logins; no hidden copy is left behind; a checkpoint export keeps the warning.
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO=${SILO:-$HOME/code/projects/silo-wt-targets/T14/debug/silo}
OUT=${OUT:-$(mktemp -d "${TMPDIR:-/tmp}/e2e-t14.XXXXXX")}
RUNTIME="$HOME/Library/Application Support/org.silo.dev/runtime/microsandbox"
SRC=e2e-t14-src; IMP=e2e-t14-imp
DEFAULT_FILE="$OUT/default.silo-backup"; INCLUDE_FILE="$OUT/include.silo-backup"
PASS=0; FAILN=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
xr() { local c=$1; shift; "$SILO" exec "$c" --timeout 170 -- sudo bash -c "$*"; }

cleanup() {
  say "cleanup (drops only the e2e-t14-* computers this run made, and its files)"
  for c in $SRC $IMP; do "$SILO" stop "$c" >/dev/null 2>&1; done
  for round in 1 2 3; do
    for c in $SRC $IMP; do "$SILO" status "$c" >/dev/null 2>&1 && "$SILO" drop "$c" --force >/dev/null 2>&1; done
    sleep 2
  done
  rm -rf "$OUT"
  say "silo list after cleanup:"; "$SILO" list
  say "hidden copies left in the runtime: $(ls "$RUNTIME/sandboxes" 2>/dev/null | grep -c '^silo-export-')"
}
trap cleanup EXIT

M1=$(LC_ALL=C tr -dc 'a-f0-9' </dev/urandom | head -c 640)
M2=$(LC_ALL=C tr -dc 'a-f0-9' </dev/urandom | head -c 640)
LOGIN_PATHS="/root/.claude/.credentials.json /root/.claude.json /root/.codex/auth.json /root/.gemini/oauth_creds.json /root/.gemini/gemini-credentials.json /root/.config/gh/hosts.yml /root/.aws/credentials /root/.docker/config.json /root/.kube/config /root/.netrc /root/.git-credentials /root/.npmrc /root/.pypirc /root/.ssh/id_ed25519 /home/silo/.claude/.credentials.json /home/silo/.claude.json /home/silo/.codex/auth.json /home/silo/.ssh/id_rsa /home/silo/project/server.pem"

scan() { # scan FILE MARKER... : per marker "FOUND"/"absent", the tar entries (disk layers) that hold it, and the bytes scanned
python3 -I - "$@" <<'PY'
import io, json, struct, subprocess, sys, tarfile
path, markers = sys.argv[1], [m.encode() for m in sys.argv[2:]]
found = {m: [] for m in markers}
scanned = 0
with open(path, 'rb') as f:
    head = f.read(16 + 4 + 8)
    assert head[:11] == b'SILO-BACKUP', 'not an export file'
    (mlen,) = struct.unpack('>Q', head[20:28])
    manifest = json.loads(f.read(mlen))
    offset = 28 + mlen
    for computer in manifest['computers']:
        size = computer['payloadSize']
        f.seek(offset)
        # The payload is a zstd-compressed tar, read here as the import reads it.
        decoded = subprocess.run(['zstd', '-dc'], input=f.read(size), capture_output=True)
        assert decoded.returncode == 0, 'zstd could not read the payload'
        data = decoded.stdout
        scanned += len(data)
        for m in markers:
            if m in data and not found[m]:
                found[m].append('(somewhere in the decompressed payload)')
        with tarfile.open(fileobj=io.BytesIO(data)) as tar:
            for member in tar:
                if not member.isfile():
                    continue
                stored = sum(length for _, length in member.sparse) if member.sparse else member.size
                region = data[member.offset_data:member.offset_data + stored]
                for m in markers:
                    if m in region:
                        found[m] = [e for e in found[m] if not e.startswith('(')] + [member.name]
        offset += size
for m in markers:
    label = m.decode()[:12] + '...'
    print(label, 'FOUND in ' + ', '.join(found[m]) if found[m] else 'absent')
print('decompressed bytes scanned:', scanned)
PY
}

say "== header"
echo "branch $(git -C "$REPO" branch --show-current) commit $(git -C "$REPO" rev-parse --short HEAD); silo: $("$SILO" --version 2>&1 | head -1); host $(sw_vers -productVersion) $(uname -m); date $(date -u +%FT%TZ)"
echo "markers: M1=${M1:0:12}... (640 hex) M2=${M2:0:12}... (640 hex)"
echo "computers before: $("$SILO" list 2>/dev/null | awk 'NR>1{print $1}' | tr '\n' ' ')"
echo "zstd: $(zstd --version | head -1)"

say "== S1 a computer with fake logins at every known login path"
"$SILO" create $SRC --linux --cpus 2 --memory 2 --disk 8 >/dev/null 2>&1 && "$SILO" start $SRC >/dev/null 2>&1; check "create+start $SRC" $? ""
xr $SRC "id silo >/dev/null 2>&1 || useradd -m silo; mkdir -p /home/silo/project; for p in $LOGIN_PATHS; do mkdir -p \$(dirname \$p); printf 'fake login %s\n$M1\n' \$p > \$p; done; chown -R silo /home/silo; echo hello > /root/keep-me.txt; echo hello > /home/silo/project/keep-too.txt; sync" >/dev/null 2>&1
check "every login path holds M1" $([ "$(xr $SRC "for p in $LOGIN_PATHS; do grep -c $M1 \$p; done" | grep -c '^1$')" = "$(echo $LOGIN_PATHS | wc -w | tr -d ' ')" ]; echo $?) "$(echo $LOGIN_PATHS | wc -w | tr -d ' ') files"
xr $SRC "printf '$M2\n' > /root/m2-deleted.txt; printf '$M2\n' > /workspace/m2-deleted.txt; df -hT / /workspace | sed 's/^/df: /'; sync; rm -f /root/m2-deleted.txt /workspace/m2-deleted.txt; sync; findmnt -rn -T /workspace -o TARGET,FSTYPE,SOURCE; findmnt -rn -T / -o TARGET,FSTYPE,SOURCE" 2>&1 | sed 's/^/guest: /'
CP_BEFORE=$("$SILO" checkpoints $SRC 2>&1 | sort)

say "== S2 the default export (no --yes, no terminal: it must ask nothing)"
rm -f "$DEFAULT_FILE"; t0=$(date +%s)
"$SILO" export $SRC -o "$DEFAULT_FILE" --json </dev/null >"$OUT/s3.json" 2>"$OUT/s3.err"; rc=$?
echo "export took $(( $(date +%s) - t0 )) s; progress:"; sed 's/^/  /' "$OUT/s3.err" | head -20
check "S2 default export succeeded" $rc ""
python3 -I -c "
import json,sys
d=json.load(open(sys.argv[1]))
print('json: logins in the file =', d['logins'], '| unchecked =', d['unchecked'])
print('json: left out =', *d['leftOut'], sep='\n  ')
" "$OUT/s3.json"
echo "export size: $(stat -f %z "$DEFAULT_FILE" 2>/dev/null) bytes"
for p in $LOGIN_PATHS; do :; done
LEFT=$(python3 -I -c "import json,sys; print(len(json.load(open(sys.argv[1]))['leftOut']))" "$OUT/s3.json")
check "S2 the result names the files it left out" $([ "${LEFT:-0}" -ge 15 ]; echo $?) "$LEFT files"
say "scanning the default export (every byte of every decompressed payload)"
RESULT=$(scan "$DEFAULT_FILE" "$M1" "$M2"); echo "$RESULT" | sed 's/^/  /'
check "S2 CANARY: M1 (the logins) is not in the default export" $(echo "$RESULT" | grep -q "^${M1:0:12}\.\.\. absent"; echo $?) ""
check "S2 CANARY: M2 (the deleted file) is not in the default export" $(echo "$RESULT" | grep -q "^${M2:0:12}\.\.\. absent"; echo $?) ""

say "== S3 the export with --include-logins (positive control)"
rm -f "$INCLUDE_FILE"
"$SILO" export $SRC -o "$INCLUDE_FILE" --include-logins --yes --json >"$OUT/s4.json" 2>"$OUT/s4.err"; rc=$?
check "S3 include export succeeded" $rc ""
RESULT_INC=$(scan "$INCLUDE_FILE" "$M1" "$M2"); echo "$RESULT_INC" | sed 's/^/  /'
check "S3 M1 is in the export that includes logins" $(echo "$RESULT_INC" | grep -q "^${M1:0:12}\.\.\. FOUND"; echo $?) ""
echo "note: M2 in the include export: $(echo "$RESULT_INC" | grep "^${M2:0:12}" | cut -d' ' -f2) (a deleted file's bytes stay in an export that is not trimmed)"
rm -f "$INCLUDE_FILE"

say "== S4 the default export imports, starts, and its login files are gone"
"$SILO" import "$DEFAULT_FILE" --name $IMP >/dev/null 2>"$OUT/s5.err"; rc=$?
check "S4 import succeeded" $rc "$(head -c 200 "$OUT/s5.err")"
"$SILO" start $IMP >/dev/null 2>&1; check "S4 the imported computer starts" $? ""
GONE=$(xr $IMP "n=0; for p in $LOGIN_PATHS; do [ -e \$p ] && n=\$((n+1)); done; echo \$n")
check "S4 none of the login files is in the imported computer" $([ "$(echo $GONE | tr -d '[:space:]')" = 0 ]; echo $?) "present: $GONE"
check "S4 the imported computer has the other files" $(xr $IMP 'cat /root/keep-me.txt /home/silo/project/keep-too.txt' | grep -q hello; echo $?) ""
check "S4 the imported computer holds neither marker" $(xr $IMP "grep -rIl -e $M1 -e $M2 /root /home /etc /workspace /var /opt /srv 2>/dev/null | wc -l" | grep -q '^0$'; echo $?) ""
check "S4 the file carries the sender's name, not a hidden copy's" $(python3 -I - "$DEFAULT_FILE" <<'PY'
import json, struct, sys
with open(sys.argv[1], 'rb') as f:
    head = f.read(28)
    manifest = json.loads(f.read(struct.unpack('>Q', head[20:28])[0]))
names = [c['name'] for c in manifest['computers']]
sys.exit(0 if names == ['e2e-t14-src'] else 1)
PY
echo $?) ""

say "== S5 the source is untouched and no hidden copy is left"
check "S5 the source still has every login file" $([ "$(xr $SRC "n=0; for p in $LOGIN_PATHS; do [ -e \$p ] && n=\$((n+1)); done; echo \$n" | tr -d '[:space:]')" = "$(echo $LOGIN_PATHS | wc -w | tr -d ' ')" ]; echo $?) ""
CP_AFTER=$("$SILO" checkpoints $SRC 2>&1 | sort)
echo "source checkpoints before: $(echo "$CP_BEFORE" | wc -l | tr -d ' ') line(s); after: $(echo "$CP_AFTER" | wc -l | tr -d ' ') line(s)"
check "S5 the source gained no checkpoint but the fork's Fork point" $([ -z "$(comm -13 <(echo "$CP_BEFORE") <(echo "$CP_AFTER") | grep -vi 'fork point')" ]; echo $?) ""
check "S5 no silo-export- computer is left in the runtime" $([ "$(ls "$RUNTIME/sandboxes" 2>/dev/null | grep -c '^silo-export-')" = 0 ]; echo $?) ""
check "S5 the hidden copy never showed in silo list" $([ "$("$SILO" list 2>&1 | grep -c silo-export-)" = 0 ]; echo $?) ""

say "== S6 a checkpoint export keeps today's warning"
"$SILO" checkpoint $SRC --name t14 >/dev/null 2>&1
"$SILO" export $SRC --checkpoint t14 -o "$OUT/cp.silo-backup" </dev/null >/dev/null 2>"$OUT/s7.err"; rc=$?
cat "$OUT/s7.err" | head -3
check "S6 a checkpoint export is refused without --yes (its logins cannot be left out)" $([ $rc != 0 ]; echo $?) "(exit $rc)"

echo
echo "RESULT: $PASS passed, $FAILN failed"
if echo "$RESULT" | grep -q FOUND; then
  echo "CANARY: the markers are still in the default export (layers named above). Silo removes the login files from the exported computer"
  echo "        (S4 passes) but does not claim the bytes are out of the file: the app and the CLI say traces may remain."
fi
exit $FAILN
