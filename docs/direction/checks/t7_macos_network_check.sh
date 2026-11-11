#!/usr/bin/env bash
# T7 live check: a macOS computer an agent may have used never boots on Apple's own network. Forks
# and imports are prepared offline and boot behind the computer's gateway; Recovery gets no
# network card; only a clean first install and a template copy use Apple's NAT.
# Runs against the Dev engine (~/.silo-dev) with throwaway e2e-t7-* computers. Run:
#   bash docs/direction/checks/t7_macos_network_check.sh 2>&1 | tee docs/direction/evidence/T7.txt
# Needs a Dev app bundle built from this tree (BUNDLE: `npm --prefix app/SiloUI run
# desktop:build:debug` after `npm --prefix app/SiloUI run runtime:prepare`) and its debug `silo`
# (SILO). The script starts the bundle's `silod --agent` by hand when no Dev engine runs, and stops
# it at the end. It never touches another engine: it refuses to run when a Dev engine it did not
# start answers. About 60 GB of disk must be free (the export file and the import are real copies).
#
# The source is the complete macOS computer e2e-t7-base. When it does not exist the script makes it
# from the template (that is also step S5's evidence) and saves a checkpoint of it, "cp-stale", whose
# record says its developer tools are an old version (the file edit is on a throwaway computer's
# checkpoint, and is the only way to get stale tools without waiting for a new build).
# DROP_BASE=1 deletes e2e-t7-base at the end; without it the base stays for another run.
#
# What it shows, in order:
#   S1  fork the stale checkpoint: no NAT boot in the setup log or on the host's lease list, the
#       setup runs behind the gateway (key login, gateway address, gateway route), the tools are
#       brought up to date, the fork ends complete
#   S3  a fork or import of a disk that lacks one bootstrap file is refused, the disk is unchanged
#   S2  export the stale checkpoint and import it: same as S1, including the tools copy over the
#       gateway's SSH port and the Claude Code install
#   S4  Recovery has no network card (needs a person at the screen: skipped)
#   S5  a template copy and a first install still use Apple's NAT (the first install is skipped)
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
T=${T:-$REPO/app/SiloUI/target}
SILO=${SILO:-$T/debug/silo}
BUNDLE=${BUNDLE:-$T/debug/bundle/macos/Silo Dev.app}
SILOD="$BUNDLE/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod"
LOG=${LOG:-$REPO/app/SiloUI/src-tauri/target/verification/silod-dev-t7.log}
SOCK="$HOME/.silo-dev/engine.sock"
DATA="$HOME/Library/Application Support/org.silo.dev"
MACS="$DATA/macos-computers"
RLOGS="$HOME/Library/Logs/org.silo.dev/macos-computers"
# STEPS picks the parts to run (S3 needs S1's fork). EXPORT_ROOT is where the export file goes: the
# import stages 65 GiB beside the 21 GiB file, so on a disk with less than about 90 GiB free it has to
# be another volume.
STEPS=${STEPS:-"S1 S3 S2 S5"}
EXPORT_ROOT=${EXPORT_ROOT:-${TMPDIR:-/tmp}}
want() { case " $STEPS " in *" $1 "*) return 0;; esac; return 1; }
BASE=e2e-t7-base; F1=e2e-t7-f1; F4=e2e-t7-f4; IMP=e2e-t7-i1; TPL=e2e-t7-tpl
MINE="$F4 $F1 $IMP $TPL"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/e2e-t7.XXXXXX")
PASS=0; FAILN=0; SKIPN=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
skip() { local name=$1; shift; echo "[$(ts)] SKIPPED  $name  -- $*"; SKIPN=$((SKIPN+1)); }
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
jfield() { python3 -I -c 'import json,sys; v=json.load(sys.stdin)
for k in sys.argv[1].split("."): v=(v or {}).get(k) if isinstance(v,dict) else None
print("" if v is None else v)' "$1"; }
cid() { "$SILO" --json status "$1" 2>/dev/null | jfield id; }
cstate() { "$SILO" --json status "$1" 2>/dev/null | jfield state; }
cdetail() { "$SILO" --json status "$1" 2>/dev/null | jfield detail; }
exists() { "$SILO" status "$1" >/dev/null 2>&1; }
cdir() { echo "$MACS/$(cid "$1")"; }
crec() { jfield "$2" <"$(cdir "$1")/computer.json"; }
clog() { echo "$RLOGS/$(cid "$1")/runtime.log"; }
# Waits until the computer leaves its setup states (stopped, running or failed), at most $2 seconds.
wait_settled() {
  local end=$((SECONDS+$2)) st gone=0
  while [ $SECONDS -lt $end ]; do
    st=$(cstate "$1"); case "$st" in stopped|running|failed) return 0;; esac
    if [ -z "$st" ]; then gone=$((gone+1)); [ $gone -ge 3 ] && return 1; else gone=0; fi
    sleep 4
  done
  return 1
}
inside() { local c=$1; shift; "$SILO" exec "$c" --timeout 170 -- bash -c "$*" 2>&1 | sed -u '/^step [0-9]*$/d'; return "${PIPESTATUS[0]}"; }
free_gib() { df -k / | awk 'NR==2{printf "%d", $4/1048576}'; }
# The Mac's NAT lease for a hardware address (empty when the host never leased one to it).
lease_of() { python3 -I -c '
import re,sys
mac=":".join(str(int(p,16)) if False else p.lstrip("0") or "0" for p in sys.argv[1].lower().split(":"))
text=open("/var/db/dhcpd_leases").read()
for block in re.findall(r"\{[^}]*\}", text):
    hw=re.search(r"hw_address=1,([0-9a-f:]+)", block); ip=re.search(r"ip_address=([0-9.]+)", block)
    if hw and ip and hw.group(1)==mac: print(ip.group(1))' "$1"; }
# Background watcher for NAT leases of $1 (a hardware address), appended to $2 once a second.
watch_lease() { ( while :; do l=$(lease_of "$1"); [ -n "$l" ] && echo "$l" >>"$2"; sleep 1; done ) >/dev/null 2>&1 & echo $!; }
# Background watcher for connections the engine's tools to the gateway's loopback listeners.
watch_gateway() {
  ( while :; do
      ports=$(lsof -nP -a -iTCP -sTCP:LISTEN -c silo-macos-gateway 2>/dev/null | awk 'NR>1{n=split($9,a,":"); print a[n]}' | sort -u)
      for p in $ports; do
        lsof -nP -a -iTCP@127.0.0.1:$p -sTCP:ESTABLISHED 2>/dev/null | awk -v p=$p 'NR>1 && $1 !~ /^silo-mac/ {print $1, "-> 127.0.0.1:" p}'
      done | sort -u >>"$1"
      sleep 1
    done ) >/dev/null 2>&1 & echo $!; }
# Disk helpers: attach a stopped computer's disk on the host, mount its Data volume and run work.
data_volume_of() { # whole disk -> Data volume device
  local cont; cont=$(diskutil info -plist "${1}s2" | plutil -extract APFSContainerReference raw -) || return 1
  diskutil apfs list -plist "$cont" | python3 -I -c '
import plistlib,sys
for c in plistlib.loads(sys.stdin.buffer.read())["Containers"]:
    for v in c["Volumes"]:
        if "Data" in v.get("Roles",[]): print(v["DeviceIdentifier"])'
}
# with_data <disk.img> <ro|rw> <command...>; the command runs with $MP set to the Data volume.
with_data() {
  local img=$1 mode=$2; shift 2
  local flag=-readonly; [ "$mode" = rw ] && flag=-readwrite
  local out whole data rc
  out=$(hdiutil attach $flag -nomount "$img") || return 1
  whole=$(echo "$out" | awk 'NR==1{print $1}')
  data=$(data_volume_of "$whole"); MP=$(mktemp -d "$WORK/mp.XXXXXX")
  if diskutil mount $([ "$mode" = ro ] && echo readOnly) -mountPoint "$MP" "/dev/$data" >/dev/null; then ( cd "$MP" && "$@" ); rc=$?; else rc=1; fi
  diskutil unmountDisk "$whole" >/dev/null 2>&1; hdiutil detach "$whole" >/dev/null 2>&1 || hdiutil detach -force "$whole" >/dev/null 2>&1
  rmdir "$MP" 2>/dev/null
  return $rc
}
BOOT_FILES=("Library/PrivilegedHelperTools/silo-network-bootstrap" "Library/LaunchDaemons/com.silo.network-bootstrap.plist" "Library/Application Support/Silo/NetworkBootstrap/config.json")
manifest_body() {
  for f in private/etc/kcpassword private/var/db/dslocal/nodes/Default/users/silo.plist Users/silo/.ssh/authorized_keys \
           "Library/Application Support/Silo/NetworkBootstrap/config.json" "Library/Application Support/Silo/NetworkBootstrap/receipt.json" \
           Library/PrivilegedHelperTools/silo-network-bootstrap Library/LaunchDaemons/com.silo.network-bootstrap.plist private/etc/ssh/ssh_host_ed25519_key.pub; do
    if [ -e "$f" ]; then echo "$(shasum -a 256 "$f" | cut -c1-16) $f"; else echo "missing $f"; fi
  done
}
# The checksums of the files a fork or import would change (account, key, host keys, bootstrap).
manifest() { with_data "$1" ro manifest_body; }
remove_boot_file() { cp -p "$1" "$WORK/saved-bootstrap-file" && rm -f "$1"; }
restore_boot_file() { mkdir -p "$(dirname "$1")" && cp -p "$WORK/saved-bootstrap-file" "$1"; }

cleanup() {
  say "cleanup (drops only the e2e-t7-* computers this run made, then stops the Dev engine it started)"
  [ -n "${LEASE_PID:-}" ] && kill "$LEASE_PID" 2>/dev/null
  [ -n "${GW_PID:-}" ] && kill "$GW_PID" 2>/dev/null
  if engine_answers; then
    local drop="$MINE"; [ "${DROP_BASE:-0}" = 1 ] && drop="$MINE $BASE"
    for c in $drop; do "$SILO" stop "$c" >/dev/null 2>&1; done
    for round in 1 2 3; do
      for c in $drop; do exists "$c" && "$SILO" drop "$c" --force >/dev/null 2>&1; done
      sleep 2
    done
    say "silo list after cleanup:"; "$SILO" list
  fi
  stop_engine && say "Dev engine stopped" || say "Dev engine did not stop in time"
  rm -rf "$WORK" "${EXPORT_DIR:-/nonexistent}"
  hdiutil info 2>/dev/null | grep -q "macos-computers" && say "WARNING: a computer's disk is still attached to this Mac"
}
trap cleanup EXIT

say "== header"
echo "commit $(cd "$REPO" && git rev-parse --short HEAD) on $(cd "$REPO" && git branch --show-current); $("$SILO" --version 2>&1); host $(sw_vers -productVersion) $(uname -m); $(date -u +%FT%TZ)"
if engine_answers && [ -z "$(our_engine)" ]; then
  echo "A Dev engine this script did not start is answering; refusing to touch it." >&2
  trap - EXIT; rm -rf "$WORK"; exit 2
fi
# A copy whose disk could not be expanded (the Recovery partition sits behind the container) keeps
# the file's length but records the container's size, which export refuses ("the disk component size
# does not match diskGiB"). Before the engine starts, the base and its checkpoints record the file's
# length, so they can be exported. This does not hide the refusal: it is the existing disk-size bug.
fix_base_disk_record() {
  python3 -I - "$MACS" "$BASE" <<'PY'
import json, os, sys
macs, name = sys.argv[1], sys.argv[2]
GIB = 1 << 30
for d in os.listdir(macs):
    p = os.path.join(macs, d, "computer.json")
    if not os.path.isfile(p):
        continue
    v = json.load(open(p))
    if v.get("name") != name:
        continue
    gib = os.path.getsize(os.path.join(macs, d, "disk.img")) // GIB
    changed = []
    if v.get("diskGiB") != gib:
        changed.append("computer %s -> %s" % (v.get("diskGiB"), gib)); v["diskGiB"] = gib
        json.dump(v, open(p, "w"), indent=2)
    cps = os.path.join(macs, d, "checkpoints")
    for c in os.listdir(cps) if os.path.isdir(cps) else []:
        cp = os.path.join(cps, c, "checkpoint.json")
        if os.path.isfile(cp):
            w = json.load(open(cp))
            if w.get("diskGib") != gib:
                changed.append("checkpoint %s %s -> %s" % (c[:8], w.get("diskGib"), gib)); w["diskGib"] = gib
                json.dump(w, open(cp, "w"), indent=2)
    print("disk record of %s now %s GiB%s" % (name, gib, (" (" + "; ".join(changed) + ")") if changed else " (unchanged)"))
PY
}
[ -n "$(our_engine)" ] || { fix_base_disk_record; start_engine; check "the Dev engine starts from the bundle" $? "pid $(our_engine), log $LOG"; }
echo "computers before: $("$SILO" list 2>/dev/null | awk 'NR>1{print $1}' | tr '\n' ' ')"
for c in $MINE; do exists "$c" && { echo "$c already exists; refusing to run" >&2; trap - EXIT; stop_engine; rm -rf "$WORK"; exit 2; }; done
[ "$(free_gib)" -ge 60 ] || { echo "only $(free_gib) GiB free; this run needs about 60" >&2; exit 2; }
echo "free disk: $(free_gib) GiB"

say "== setup: the source computer $BASE and its stale checkpoint"
if ! exists $BASE; then
  "$SILO" create $BASE --macos --cpus 4 --memory 8 --disk 40 >/dev/null 2>&1; check "create $BASE from the template" $? ""
  wait_settled $BASE 900; check "$BASE finishes its setup" $([ "$(cstate $BASE)" = stopped ]; echo $?) "$(cstate $BASE)"
  "$SILO" checkpoint $BASE --name cp-stale >/dev/null 2>&1; check "checkpoint cp-stale of $BASE" $? ""
  cpid=$("$SILO" checkpoints $BASE | awk '$2=="cp-stale"{print $1}')
  cpjson="$(cdir $BASE)/checkpoints/$cpid/checkpoint.json"
  python3 -I - "$cpjson" <<'PY'
import json, sys
p = sys.argv[1]
v = json.load(open(p))
v["developerToolsVersion"] = "stale-tools-0.0.1"
json.dump(v, open(p, "w"), indent=2)
PY
  check "cp-stale's record names an old tools version" $? ""
fi
CP=$("$SILO" checkpoints $BASE | awk '$2=="cp-stale"{print $1}')
[ -n "$CP" ] || { echo "$BASE has no cp-stale checkpoint" >&2; exit 2; }
STALE=$(jfield developerToolsVersion <"$(cdir $BASE)/checkpoints/$CP/checkpoint.json")
CURRENT=$(crec $BASE developerToolsVersion)
echo "$BASE: state $(cstate $BASE), tools version now $CURRENT; checkpoint cp-stale ($CP) says tools version $STALE"
check "the source is complete and its checkpoint's tools are stale" $([ "$(cstate $BASE)" = stopped ] && [ -n "$CURRENT" ] && [ "$STALE" != "$CURRENT" ]; echo $?) ""

# Everything a computer made from stale tools must show, whether it is a fork or an import.
# $1 name, $2 label
verify_behind_gateway() {
  local c=$1 label=$2 id mac log
  id=$(cid "$c"); mac=$(crec "$c" macAddress); log=$(clog "$c")
  echo "--- setup log of $c:"; grep -E "step:|boot [0-9]+:|setup (started|finished|failed)" "$log" | cut -c1-200 | sed 's/^/  /'
  check "$label the setup ended complete (stopped, no failure, all steps done, not imported, first Start done)" \
    $([ "$(cstate "$c")" = stopped ] && [ -z "$(cdetail "$c")" ] && [ "$(crec "$c" imported)" = False ] && [ "$(crec "$c" firstBoot)" = False ] \
      && [ "$(crec "$c" setup.proxyBootstrap)" = True ] && [ "$(crec "$c" setup.developerTools)" = True ] && [ "$(crec "$c" setup.computerUse)" = True ] \
      && [ "$(crec "$c" setup.needsPersonalizing)" = False ]; echo $?) "state $(cstate "$c"); setup $(jfield setup <"$(cdir "$c")/computer.json" | tr -d '\n')"
  check "$label the tools were brought up to date in the guest (version $STALE -> $(crec "$c" developerToolsVersion))" \
    $([ "$(crec "$c" developerToolsVersion)" = "$CURRENT" ] && grep -q "step: Installing developer tools" "$log"; echo $?) ""
  check "$label the setup log has no NAT boot (no setup boot, no first-install boot)" \
    $(! grep -qE "setup boot|Starting macOS for the first time" "$log"; echo $?) "boots: $(grep -oE 'boot [0-9]+: [A-Za-z ]+' "$log" | tr '\n' ';')"
  check "$label the guest came up on the first Start behind the gateway" $(grep -q "boot [0-9]*: first Start" "$log"; echo $?) ""
}
wait_stopped() { local end=$((SECONDS+$2)); while [ $SECONDS -lt $end ]; do [ "$(cstate "$1")" = stopped ] && return 0; sleep 2; done; return 1; }
# Starts $1 and shows its key login, its gateway address and route, and its tools.
verify_running() {
  local c=$1 label=$2 id mac gip gw out
  id=$(cid "$c"); mac=$(crec "$c" macAddress)
  "$SILO" start "$c" >/dev/null 2>&1; check "$label start $c" $? "$(cstate "$c")"
  gip=$(jfield guestIpv4 <"$MACS/$id/proxy-bootstrap.json"); gw=$(jfield gatewayIpv4 <"$MACS/$id/proxy-bootstrap.json")
  out=$(inside "$c" 'whoami; sudo -n true && echo sudo-ok')
  check "$label the engine logs in with the computer's key (exec as silo) and sudo -n works" $([ "$out" = "$(printf 'silo\nsudo-ok')" ]; echo $?) "$(echo "$out" | tr '\n' ' ')"
  out=$(inside "$c" 'ifconfig | awk "/inet /&&\$2!=\"127.0.0.1\"{print \$2}"; route -n get default | awk "/gateway:/{print \$2}"')
  echo "address and gateway inside: $(echo "$out" | tr '\n' ' ') (recorded: $gip via $gw)"
  check "$label its only address is the gateway's ($gip), its route goes to $gw" $([ "$out" = "$(printf '%s\n%s' "$gip" "$gw")" ]; echo $?) ""
  check "$label no Apple NAT address (192.168.64.x) inside" $(! echo "$out" | grep -q '^192\.168\.64\.'; echo $?) ""
  out=$(inside "$c" 'export PATH=$PATH:$HOME/.local/bin:/usr/local/bin; claude --version; node --version; git --version | head -1; grep -c claudeVersion "/Library/Application Support/Silo/developer-tools-receipt.json"')
  echo "tools inside: $(echo "$out" | tr '\n' '|')"
  check "$label Claude Code, Node and Git run, and the tools receipt names Claude Code" $(echo "$out" | grep -q 'Claude Code' && echo "$out" | grep -q '^v' && echo "$out" | grep -q 'git version' && [ "$(echo "$out" | tail -1)" = 1 ]; echo $?) ""
  "$SILO" stop "$c" >/dev/null 2>&1; wait_stopped "$c" 120
}

if want S1; then
say "== S1 fork a complete computer whose tools are stale"
cp /var/db/dhcpd_leases "$WORK/leases-before" 2>/dev/null
"$SILO" fork $BASE --checkpoint "$CP" --name $F1 --no-start >/dev/null 2>&1; check "S1 fork $BASE at cp-stale" $? ""
MAC1=$(crec $F1 macAddress); LEASES1="$WORK/leases-f1"; : >"$LEASES1"; LEASE_PID=$(watch_lease "$MAC1" "$LEASES1")
wait_settled $F1 600; check "S1 the fork's setup settles" $? "state $(cstate $F1)"
verify_behind_gateway $F1 "S1"
verify_running $F1 "S1"
kill "$LEASE_PID" 2>/dev/null; LEASE_PID=
check "S1 the Mac's NAT never leased an address to the fork ($MAC1) during its setup and Start" $([ ! -s "$LEASES1" ] && [ -z "$(lease_of "$MAC1")" ]; echo $?) "leases seen: $(sort -u "$LEASES1" | tr '\n' ' ')"
mb=$(manifest "$(cdir $BASE)/disk.img"); mf=$(manifest "$(cdir $F1)/disk.img")
check "S1 control: the fork's own login files differ from its source's (the offline setup did run)" $([ "$(echo "$mb" | grep kcpassword)" != "$(echo "$mf" | grep kcpassword)" ]; echo $?) "kcpassword source $(echo "$mb" | grep kcpassword | cut -c1-16), fork $(echo "$mf" | grep kcpassword | cut -c1-16)"

fi
if want S3; then
say "== S3 a fork of a disk that lacks one bootstrap file is refused and the disk is left as it was"
SRC_DISK="$(cdir $F1)/disk.img"
for f in "${BOOT_FILES[@]}"; do
  short=$(basename "$f")
  with_data "$SRC_DISK" rw remove_boot_file "$f"; check "S3 remove $short from $F1's disk" $? ""
  before=$(manifest "$SRC_DISK")
  echo "$before" | grep -q "^missing $f"; check "S3 the disk lacks only $short" $([ "$(echo "$before" | grep -c '^missing ')" = 1 ] && echo "$before" | grep -q "^missing $f"; echo $?) ""
  "$SILO" fork $F1 --name $F4 --no-start >/dev/null 2>&1; check "S3 fork $F1 (without $short) is accepted for setup" $? ""
  wait_settled $F4 300
  d=$(cdetail $F4)
  echo "state $(cstate $F4); detail: $d"
  check "S3 the fork is refused: failed, naming the missing network setup" $([ "$(cstate $F4)" = failed ] && echo "$d" | grep -q "does not have the network setup"; echo $?) ""
  check "S3 the refused fork never booted" $(! grep -qE "boot [0-9]+:" "$(clog $F4)"; echo $?) ""
  after=$(manifest "$(cdir $F4)/disk.img")
  check "S3 the refused disk is unchanged (account, key, host key, bootstrap files all as in the source)" $([ "$before" = "$after" ]; echo $?) "$(echo "$after" | grep -cv '^missing') files, same checksums"
  "$SILO" drop $F4 --force >/dev/null 2>&1; check "S3 drop the refused fork" $? ""
  with_data "$SRC_DISK" rw restore_boot_file "$f"; check "S3 put $short back" $? ""
done
"$SILO" drop $F1 --force >/dev/null 2>&1; check "S1/S3 drop $F1" $? ""

fi
if want S2; then
say "== S2 export the stale checkpoint and import it"
EXPORT_DIR=$(mktemp -d "$EXPORT_ROOT/e2e-t7-export.XXXXXX"); FILE="$EXPORT_DIR/e2e-t7.silo-backup"
same_volume=$([ "$(stat -f %d "$EXPORT_DIR")" = "$(stat -f %d "$MACS")" ] && echo yes || echo no)
echo "export file goes to $EXPORT_DIR (same disk as the Dev data: $same_volume); free disk: $(free_gib) GiB"
KEEP_FILE=0
if [ -n "${EXPORT_FILE:-}" ] && [ -f "$EXPORT_FILE" ]; then
  FILE=$EXPORT_FILE; KEEP_FILE=1; rmdir "$EXPORT_DIR" 2>/dev/null; EXPORT_DIR=
  echo "using the export file $FILE ($(ls -l "$FILE" | awk '{printf "%.1f GiB", $5/1073741824}')) made by an earlier run; the export itself is not repeated"
elif [ "$same_volume" = yes ] && [ "$(free_gib)" -lt 90 ]; then
  skip "S2 export and import" "the import stages 65 GiB beside the 21 GiB export file; this disk has $(free_gib) GiB free. Run with EXPORT_ROOT on another volume"
  FILE=
else
  "$SILO" export $BASE --checkpoint "$CP" -o "$FILE" --yes >"$WORK/export.out" 2>&1; check "S2 export $BASE at cp-stale" $? "$(tail -1 "$WORK/export.out") $(ls -l "$FILE" 2>/dev/null | awk '{printf "%.1f GiB", $5/1073741824}')"
  [ -f "$FILE" ] || FILE=
fi
if [ -n "$FILE" ]; then
  GW_FILE="$WORK/gateway-connections"; : >"$GW_FILE"; GW_PID=$(watch_gateway "$GW_FILE")
  # The export's staging space comes back to the disk a while after it ends; the import needs 65 GiB.
  for _ in $(seq 1 60); do [ "$(free_gib)" -ge 72 ] && break; sleep 5; done
  echo "free disk before the import: $(free_gib) GiB"
  "$SILO" import "$FILE" --name $IMP >"$WORK/import.out" 2>&1; irc=$?
  check "S2 import the file as $IMP" $irc "$(tail -1 "$WORK/import.out" | cut -c1-300)"
  if [ $KEEP_FILE = 0 ]; then rm -f "$FILE"; rmdir "$EXPORT_DIR" 2>/dev/null; echo "export file deleted; free disk: $(free_gib) GiB"; fi
  if [ $irc = 0 ] && exists $IMP; then
    MAC2=$(crec $IMP macAddress); LEASES2="$WORK/leases-i1"; : >"$LEASES2"; LEASE_PID=$(watch_lease "$MAC2" "$LEASES2")
    echo "after the import: state $(cstate $IMP), imported $(crec $IMP imported), needs personalizing $(crec $IMP setup.needsPersonalizing)"
    "$SILO" retry-setup $IMP >"$WORK/retry.out" 2>&1; check "S2 Retry setup finishes the imported computer" $? "$(tail -1 "$WORK/retry.out" | cut -c1-200)"
    end=$((SECONDS+900)); until grep -qE "setup (finished|failed)" "$(clog $IMP)" 2>/dev/null || [ $SECONDS -gt $end ]; do sleep 5; done
    wait_settled $IMP 120; check "S2 the import's setup settles" $? "state $(cstate $IMP)"
    kill "$GW_PID" 2>/dev/null; GW_PID=
    verify_behind_gateway $IMP "S2"
    grep -q "step: Personalizing the imported computer" "$(clog $IMP)"; check "S2 the import was prepared offline on its stopped disk" $? ""
    verify_running $IMP "S2"
    kill "$LEASE_PID" 2>/dev/null; LEASE_PID=
    check "S2 the Mac's NAT never leased an address to the import ($MAC2)" $([ ! -s "$LEASES2" ] && [ -z "$(lease_of "$MAC2")" ]; echo $?) "leases seen: $(sort -u "$LEASES2" | tr '\n' ' ')"
    tools_mb=$(du -sk "$DATA/macos-developer-tools" 2>/dev/null | awk '{printf "%.0f", $1/1024}')
    echo "connections to the gateway's loopback listeners during the import:"; sort -u "$GW_FILE" | sed 's/^/  /'
    check "S2 the tools ($tools_mb MB in the engine's cache) were copied over the gateway's SSH port (an ssh/scp connection to a gateway listener was seen)" $(grep -qE "^(scp|ssh|sftp|silod)" "$GW_FILE"; echo $?) "$(sort -u "$GW_FILE" | head -3 | tr '\n' ';')"
    "$SILO" drop $IMP --force >/dev/null 2>&1; check "S2 drop $IMP" $? ""
  else
    [ -n "${GW_PID:-}" ] && kill "$GW_PID" 2>/dev/null; GW_PID=
  fi
fi
fi

say "== S4 Recovery has no network card"
skip "S4 Recovery with no network card" "Recovery starts only inside a fresh install's SIP step, which drives the visible Setting up window and reads the screen; no engine method starts a finished computer in Recovery, and every Dev computer here already has SIP off (csrutil status: disabled in the setup log of $BASE). Covered by the unit test engine::tests::only_the_bootstrap_boots_get_apple_network_and_recovery_gets_none"

if want S5; then
say "== S5 a template copy still uses Apple's NAT; a first install too"
skip "S5 first install" "a macOS template already exists on the Dev engine, so a new computer copies it; a first install runs only when no template matches and installs from the restore image, which takes hours and would replace the Dev engine's template"
"$SILO" create $TPL --macos --cpus 4 --memory 8 --disk 40 >/dev/null 2>&1; check "S5 create $TPL from the template" $? ""
MAC3=$(crec $TPL macAddress); LEASES3="$WORK/leases-tpl"; : >"$LEASES3"; LEASE_PID=$(watch_lease "$MAC3" "$LEASES3")
wait_settled $TPL 900; check "S5 the copy's setup settles" $? "state $(cstate $TPL)"
kill "$LEASE_PID" 2>/dev/null; LEASE_PID=
echo "--- setup log of $TPL:"; grep -E "step:|boot [0-9]+:|setup (started|finished|failed)" "$(clog $TPL)" | cut -c1-200 | sed 's/^/  /'
check "S5 the template copy's setup boots were setup boots (Apple NAT)" $(grep -q "boot [0-9]*: setup boot" "$(clog $TPL)"; echo $?) "setup boots: $(grep -c 'setup boot' "$(clog $TPL)")"
check "S5 the Mac's NAT leased the copy ($MAC3) an address while it was set up" $([ -s "$LEASES3" ]; echo $?) "leases seen: $(sort -u "$LEASES3" | tr '\n' ' ')"
check "S5 the copy ended complete" $([ "$(cstate $TPL)" = stopped ] && [ -z "$(cdetail $TPL)" ] && [ "$(crec $TPL setup.proxyBootstrap)" = True ]; echo $?) "state $(cstate $TPL)"
"$SILO" start $TPL >/dev/null 2>&1
out=$(inside $TPL 'ifconfig | awk "/inet /&&\$2!=\"127.0.0.1\"{print \$2}"')
gip=$(jfield guestIpv4 <"$(cdir $TPL)/proxy-bootstrap.json")
check "S5 and its first Start after setup is behind the gateway ($gip), not on NAT" $([ "$out" = "$gip" ]; echo $?) "$out"
"$SILO" stop $TPL >/dev/null 2>&1; wait_stopped $TPL 120
"$SILO" drop $TPL --force >/dev/null 2>&1; check "S5 drop $TPL" $? ""

fi

say "== summary: $PASS passed, $FAILN failed, $SKIPN skipped"
