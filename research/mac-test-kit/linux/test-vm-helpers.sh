#!/bin/bash
# Linux checks of the VM helper and the tests built on it (00, 01, 10, 13), for linux/selftest.sh.
#   1. swift/Core.swift (the part of silovm that needs only Foundation) is compiled with a Swift
#      toolchain when one is found (SWIFTC, or swiftc on PATH) and its checks run, including its
#      ctl client against the fake's control socket. Without a toolchain this part is skipped.
#   2. lib/vm.sh's functions run against linux/fake-silovm, with linux/fake-bin/ssh on PATH.
#   3. Each test runs with KIT_FAKE=1 in each scenario of the fake, and must end on the outcome
#      that scenario implies.
# Everything is written under a temporary folder, deleted at the end. Exit 1 on the first failure.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
KIT_ROOT="$(cd "$HERE/.." && pwd)"
export KIT_ROOT

T="$(mktemp -d "${TMPDIR:-/tmp}/silo-vmtest.XXXXXX")"
SERVER_PID=""
cleanup() {
  local b
  for b in "$T"/root/work/*/*.silovm; do
    [ -f "$b/run.pid" ] && kill -9 "$(cat "$b/run.pid")" 2>/dev/null
  done
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null
  rm -rf "$T"
}
trap cleanup EXIT

export KIT_WORK_ROOT="$T/root"
export KIT_FAKE=1 KIT_YES=1 KIT_SUDO=0 KIT_KEEP_VMS=0
export PATH="$HERE/fake-bin:$PATH"
export FAKE_SILOVM_HOST_USER=ana
export KIT_OWN_NAME=ana
export KIT_SSH_LIMIT="${KIT_SSH_LIMIT:-8}"
mkdir -p "$KIT_WORK_ROOT/bin"
cp "$HERE/fake-silovm" "$KIT_WORK_ROOT/bin/silovm"
chmod +x "$KIT_WORK_ROOT/bin/silovm" "$HERE/fake-bin/ssh"

pass=0
ok() { pass=$((pass + 1)); echo "ok   $*"; }
bad() { echo "FAIL $*" >&2; exit 1; }

# reset_fake: forget every fake computer's address and the start counter.
reset_fake() { rm -rf "$KIT_WORK_ROOT/fake-net"; }

# run_test ID LABEL [VAR=value...]: run one test in a fresh result file; prints the outcome key.
run_test() {
  local id="$1" label="$2"; shift 2
  local out="$T/results-$id-$label.jsonl" f
  f="$(ls "$KIT_ROOT"/tests/"$id"-*.sh)"
  : > "$out"
  reset_fake
  rm -rf "$KIT_WORK_ROOT/work/$id"; mkdir -p "$KIT_WORK_ROOT/work/$id"
  env "$@" KIT_ID="$id" KIT_WORK="$KIT_WORK_ROOT/work/$id" KIT_OUT="$out" bash "$f" < /dev/null > "$T/log-$id-$label.txt" 2>&1
  local rc=$?
  python3 - "$out" "$rc" <<'EOF'
import json, sys
key = None
for line in open(sys.argv[1]):
    e = json.loads(line)
    if e["kind"] == "outcome": key = e["key"]
    if e["kind"] in ("skip", "error"): key = e["kind"] + ":" + e.get("reason", "")
print(key or "none(exit %s)" % sys.argv[2])
EOF
}

expect() { # expect ID LABEL WANT [VAR=value...]; WANT ending in * matches a prefix
  local id="$1" label="$2" want="$3"; shift 3
  local got
  got="$(run_test "$id" "$label" "$@")"
  # shellcheck disable=SC2254
  case "$got" in $want) local match=1 ;; *) local match=0 ;; esac
  if [ "$match" = 1 ]; then
    ok "test $id ($label): $got"
  else
    echo "--- log of test $id ($label):" >&2; tail -30 "$T/log-$id-$label.txt" >&2
    bad "test $id ($label): wanted $want, got $got"
  fi
}

# ------------------------------------------------------------------ 1. Swift core
SWIFTC="${SWIFTC:-$(command -v swiftc || true)}"
# On a Mac, ./kit build compiles the real silovm; the stand-ins of Apple's frameworks would clash
# with the real ones there.
if [ "$(uname -s)" = Darwin ]; then
  echo "skip swift/Core.swift checks: on a Mac, ./kit build compiles silovm itself"
elif [ -n "$SWIFTC" ] && [ -x "$SWIFTC" ]; then
  if "$SWIFTC" -parse-as-library "$KIT_ROOT/swift/Core.swift" "$HERE/swift/CoreCheck.swift" -o "$T/corecheck" > "$T/swiftc.log" 2>&1; then
    ok "swift/Core.swift compiles on Linux ($("$SWIFTC" --version 2>&1 | head -1))"
  else
    cat "$T/swiftc.log" >&2; bad "swift/Core.swift does not compile"
  fi
  r="$("$T/corecheck" selftest)" || bad "Core.swift checks: $r"
  ok "Core.swift checks: $r"
  # Machine.swift against stand-ins of the Apple frameworks, with the macOS 27 parts switched on.
  sed -e '/^import AppKit$/d' -e '/^import Darwin$/d' -e '/^import Virtualization$/d' \
      -e 's/^\( *\)#if compiler(>=6.4)$/\1#if true/' "$KIT_ROOT/swift/Machine.swift" > "$T/Machine.swift"
  if "$SWIFTC" -typecheck -parse-as-library -D SILOVM_STUBCHECK "$KIT_ROOT/swift/Core.swift" "$KIT_ROOT/swift/SiloVM.swift" \
       "$T/Machine.swift" "$HERE/swift/AppleStubs.swift" > "$T/stub.log" 2>&1 && ! grep -q 'warning:' "$T/stub.log"; then
    ok "swift/Machine.swift and SiloVM.swift typecheck against linux/swift/AppleStubs.swift, no warnings"
  else
    cat "$T/stub.log" >&2; bad "swift/Machine.swift does not typecheck against the stubs"
  fi
else
  echo "skip swift/Core.swift: no Swift toolchain (set SWIFTC)"
fi

# ------------------------------------------------------------------ 2. lib/vm.sh
(
  export KIT_ID=vm KIT_WORK="$KIT_WORK_ROOT/work/vm" KIT_OUT="$T/vm.jsonl"
  mkdir -p "$KIT_WORK"; : > "$KIT_OUT"
  # shellcheck disable=SC1091
  . "$KIT_ROOT/lib/common.sh"
  # shellcheck disable=SC1091
  . "$KIT_ROOT/lib/vm.sh"
  # shellcheck disable=SC1091
  . "$KIT_ROOT/lib/first-start.sh"
  "$SILOVM" install --ipsw "$KIT_ROOT/kit" --out "$KIT_BASE_VM" > /dev/null 2>&1 || bad "fake install"
  b="$(vm_fresh one)" || bad "vm_fresh"
  [ -f "$b/config.json" ] || bad "vm_fresh made no bundle"
  # shellcheck disable=SC2046
  pid="$(vm_start "$b" $(vm_provision_args))"
  [ -n "$pid" ] && vm_running "$b" || bad "vm_start"
  s="$(vm_ready "$b" "$VM_USER" 20)" || bad "vm_ready"
  echo "ok   vm_ready: SSH after $s s"
  [ -n "$(vm_event "$b" started)" ] || bad "no started event"
  vm_ssh "$b" "$VM_USER" true || bad "vm_ssh with the key"
  vm_ssh_pw "$b" root true 2>/dev/null && bad "ssh as a user that was not provisioned answered"
  if [ -n "${SWIFTC:-}" ] && [ -x "$T/corecheck" ]; then
    st="$("$T/corecheck" ctl --bundle "$b" state)" || bad "Swift ctl client against the fake: $st"
    echo "ok   Swift ctl client: $st"
    mac="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["mac"])' "$b/config.json")"
    oct="$(printf '%s' "$mac" | awk -F: '{print $1":"$2":"$3":"$4":"$5":"$6}' | python3 -c 'import sys; print(":".join("%x" % int(p, 16) for p in sys.stdin.read().strip().split(":")))')"
    printf '{\n\tname=fake\n\tip_address=10.9.8.7\n\thw_address=1,%s\n\tidentifier=1,%s\n\tlease=0x7fffffff\n}\n' "$oct" "$oct" > "$T/leases"
    ip="$(SILOVM_DHCPD_LEASES="$T/leases" "$T/corecheck" ip --bundle "$b" --wait 1)" || bad "Swift ip on a lease without leading zeros"
    [ "$ip" = 10.9.8.7 ] || bad "Swift ip gave $ip"
    echo "ok   Swift ip reads a bootpd lease ($oct -> $ip)"
  fi
  "$SILOVM" ctl --bundle "$b" screenshot "$T/x.png" > /dev/null && bad "screenshot without --gui answered ok"
  how="$(vm_stop "$b" 10)"
  [ "$how" = guest ] || bad "vm_stop: $how"
  [ "$(vm_event "$b" stopped | python3 -c 'import json,sys; print(json.load(sys.stdin)["how"])')" = requested ] || bad "stopped how"
  echo "ok   vm_stop: the guest stopped when asked"
  pid="$(FAKE_SILOVM_GUEST_IGNORES_STOP=1 vm_start "$b")"
  fs_wait_event "$b" started 10 || bad "second start"
  how="$(vm_stop "$b" 2)"
  [ "$how" = forced ] || bad "vm_stop of a guest that ignores the request: $how"
  echo "ok   vm_stop forces a guest that ignores the request"
  pid="$(vm_start "$b" --net "vfkit:$T/gv.sock" --mtu 9000)"
  fs_wait_event "$b" started 10 || bad "start with --mtu"
  [ "$(vm_event "$b" config | python3 -c 'import json,sys; print(json.load(sys.stdin)["mtu"])')" = 9000 ] || bad "--mtu not in the config event"
  vm_stop "$b" 5 > /dev/null
  "$SILOVM" run --bundle "$b" --mtu 9000 > /dev/null 2>&1 && bad "--mtu without vfkit was accepted"
  echo "ok   --mtu with vfkit only"
  vm_destroy "$b"
  [ ! -e "$b" ] || bad "vm_destroy left $b"
  b2="$(vm_fresh two --new-identity)" || bad "vm_fresh --new-identity"
  [ "$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["mac"])' "$b2/config.json")" != \
    "$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["mac"])' "$KIT_BASE_VM/config.json")" ] || bad "--new-identity kept the MAC"
  echo "ok   vm_fresh --new-identity gives a new MAC"
  vm_destroy "$b2"
) || exit 1
pass=$((pass + 1))
rm -rf "$KIT_WORK_ROOT/base"

# ------------------------------------------------------------------ 3. the tests
# 00: a download cut after 1 MB, resumed by curl's retry; then a rerun reuses the base.
head -c 3000000 /dev/urandom > "$T/restore.ipsw"
python3 "$HERE/range-server.py" "$T/restore.ipsw" "$T/port" 1000000 &
SERVER_PID=$!
i=0; while [ ! -s "$T/port" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
url="http://127.0.0.1:$(cat "$T/port")/restore.ipsw"
expect 00 fresh installed FAKE_SILOVM_IPSW_URL="$url" KIT_RETRY_DELAY=1
cmp -s "$T/restore.ipsw" "$KIT_WORK_ROOT/base/27C100.ipsw" || bad "test 00: the resumed download differs from the file"
ok "test 00: the download resumed after a cut and matches byte for byte"
expect 00 again reused FAKE_SILOVM_IPSW_URL="$url"
kill "$SERVER_PID" 2>/dev/null; SERVER_PID=""
rm -rf "$KIT_WORK_ROOT/base"
expect 00 no-server download-failed FAKE_SILOVM_IPSW_URL="http://127.0.0.1:9/none.ipsw" KIT_RETRY_DELAY=1
"$KIT_WORK_ROOT/bin/silovm" install --ipsw "$KIT_ROOT/kit" --out "$KIT_WORK_ROOT/base/installed.silovm" > /dev/null 2>&1 \
  || bad "fake install of the base"

# 01
expect 01 works own-name-works
expect 01 validate own-name-refused FAKE_SILOVM_REFUSE=validate
expect 01 set own-name-refused FAKE_SILOVM_REFUSE=set
expect 01 start own-name-refused FAKE_SILOVM_REFUSE=start
expect 01 silent own-name-silent FAKE_SILOVM_REFUSE=silent
expect 01 all both-fail FAKE_SILOVM_REFUSE=validate FAKE_SILOVM_REFUSE_ALL=1
grep -q '"name": "own_refused_stage", "value": "set"\|"own_refused_stage","value":"set"' "$T/results-01-set.jsonl" \
  || bad "test 01 (set): the refused stage was not recorded"
grep -q 'guestProvisioningInvalidUsername' "$T/results-01-start.jsonl" || bad "test 01 (start): the error name was not recorded"
ok "test 01 records the stage and the error name"
expect 01 no-api "skip:silovm cannot provision a computer here*" FAKE_SILOVM_NO_API=1

# 10
expect 10 keeps provisioned-after-cut FAKE_SILOVM_BOOT_SECONDS=4 FAKE_SILOVM_CUTOFF=keeps
expect 10 loses not-provisioned FAKE_SILOVM_BOOT_SECONDS=4 FAKE_SILOVM_CUTOFF=loses
expect 10 refused refused-second-start FAKE_SILOVM_BOOT_SECONDS=4 FAKE_SILOVM_REFUSE_RESTART=1

# 13
expect 13 reliable reliable-identity-ok KIT_FRESH_COUNT=3
expect 13 identity reliable-identity-fails KIT_FRESH_COUNT=3 FAKE_SILOVM_NEW_IDENTITY=fails
expect 13 flaky unreliable KIT_FRESH_COUNT=3 FAKE_SILOVM_FAIL_EVERY=2
expect 13 none all-failed KIT_FRESH_COUNT=2 FAKE_SILOVM_REFUSE=silent FAKE_SILOVM_REFUSE_ALL=1

leftover="$(ls -d "$KIT_WORK_ROOT"/work/*/*.silovm 2>/dev/null || true)"
[ -z "$leftover" ] || bad "computers left behind: $leftover"
ok "no computer left behind"
echo "test-vm-helpers: $pass checks passed"
