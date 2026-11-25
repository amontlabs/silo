#!/bin/bash
# Linux checks of tests 31, 32 and 33 (the checkpoint engine's prototype, NBD, TRIM), for
# linux/selftest.sh:
#   1. py/31-ckpt.py reads the prototype's committed XFS runs (linux/fixtures/31-xfs, copied from
#      research/2026-11-24-checkpoint-engine/proto/results) and gives the outcome they imply, and
#      each other outcome on runs changed to imply it.
#   2. py/32-decide.py and py/33-decide.py on made-up runs, one per outcome.
#   3. py/nbdserve.py and lib/nbd.sh: start and stop on a Unix socket and on TCP, a client's
#      handshake and writes, and linux/fake-silovm's --disk-nbd against it.
#   4. Tests 32 and 33 in their Linux analogues (root, XFS and loop devices; skipped otherwise):
#      32's checkpoint holds, and a planted bug in the server (no copy before a write) is caught.
#   5. Test 31 end to end, small, on XFS, when a Zig 0.15.2 is found (KIT_ZIG or PATH) and the
#      prototype's source is next to the kit or named by KIT_CKPT_PROTO; skipped otherwise.
# Everything is written under a temporary folder, deleted at the end. Exit 1 on the first failure.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
KIT_ROOT="$(cd "$HERE/.." && pwd)"
export KIT_ROOT
T="$(mktemp -d "${TMPDIR:-/tmp}/silo-ckpttest.XXXXXX")"
PIDS=""
cleanup() {
  local p
  for p in $PIDS; do kill "$p" 2>/dev/null; done
  rm -rf "$T"
}
trap cleanup EXIT
ok() { echo "ok   $*"; }
bad() { echo "FAIL $*" >&2; exit 1; }
key() { printf '%s\n' "$1" | awk -F'\t' '$1 == "O" {print $2}'; }

# ------------------------------------------------------------------ 1. test 31's reading
F="$HERE/fixtures/31-xfs"
out="$(python3 "$KIT_ROOT/py/31-ckpt.py" "$F")" || bad "31-ckpt.py on the XFS runs"
k="$(key "$(printf '%s\n' "$out" | tail -1)")"
[ "$k" = shared-clone-stalls ] || bad "31 on the XFS runs: wanted shared-clone-stalls (1.0 s for a shared clone), got $k"
for m in full_longest_write_ms cbw_in_clone_max_ms nbd_pause_max_ms delta_checkpoint_op_max_s full_unrelated_clone_longest_ms cbw_drop_longest_ms; do
  printf '%s\n' "$out" | awk -F'\t' -v m="$m" '$1 == "M" && $2 == m {f = 1} END {exit !f}' || bad "31: no measure $m"
done
printf '%s\n' "$out" | awk -F'\t' '$1 == "F" && $2 == "mutants" && $3 == "two-renames caught; no-tmp caught" {f = 1} END {exit !f}' || bad "31: mutants fact"
printf '%s\n' "$out" | awk -F'\t' '$1 == "M" && $3 !~ /^-?[0-9]+(\.[0-9]+)?$/ {exit 1}' || bad "31: a measure that is not a number"
ok "31-ckpt.py: the prototype's XFS runs give shared-clone-stalls, with $(printf '%s\n' "$out" | grep -c '^M') measures"
ENGINE="$KIT_ROOT/../2026-11-24-checkpoint-engine/proto/results"
if [ -d "$ENGINE" ]; then
  mkdir -p "$T/eng"
  for w in full cbw nbd delta; do [ -f "$ENGINE/$w-8g.jsonl" ] && cp "$ENGINE/$w-8g.jsonl" "$T/eng/$w.jsonl"; done
  k="$(key "$(python3 "$KIT_ROOT/py/31-ckpt.py" "$T/eng" | tail -1)")"
  [ "$k" = shared-clone-stalls ] || bad "31 on the engine's own results: got $k"
  ok "31-ckpt.py: the engine's results next to the kit read the same"
fi

variant() { # variant NAME PYTHON: a copy of the XFS runs changed by PYTHON (d: the folder)
  rm -rf "${T:?}/$1"; cp -r "$F" "$T/$1"
  python3 - "$T/$1" <<EOF || bad "variant $1"
import json, os, sys
d = sys.argv[1]
def edit(way, fn):
    p = os.path.join(d, way + ".jsonl")
    rows = [json.loads(l) for l in open(p)]
    rows = [fn(r) for r in rows]
    open(p, "w").write("".join(json.dumps(r) + "\n" for r in rows if r is not None))
$2
EOF
  key "$(python3 "$KIT_ROOT/py/31-ckpt.py" "$T/$1" | tail -1)"
}
quiet='
def q(r):
    if r.get("phase") == "unrelated-clone": r["longest_write_ms"] = 40.0
    if r.get("phase") == "ckpt": r["in_clone_ms"] = min(r["in_clone_ms"], 60.0)
    return r
for w in ("full", "cbw", "nbd"): edit(w, q)'
[ "$(variant free "$quiet")" = shared-clone-free ] || bad "31: shared-clone-free"
[ "$(variant waits "$quiet
def w(r):
    if r.get(\"phase\") == \"ckpt\": r[\"in_clone_ms\"] = 400.0
    return r
edit(\"cbw\", w)")" = shared-clone-waits ] || bad "31: shared-clone-waits"
[ "$(variant fullfree "$quiet
def f(r):
    if r.get(\"phase\") == \"ckpt\": r[\"longest_write_ms\"] = 80.0
    return r
edit(\"full\", f)")" = full-free ] || bad "31: full-free"
[ "$(variant moment '
def v(r):
    if r.get("phase") == "verify" and r["n"] == 5: r["ok"] = False
    return r
edit("cbw", v)')" = not-a-moment ] || bad "31: not-a-moment"
[ "$(variant mutant 'open(os.path.join(d, "mutants.txt"), "w").write("two-renames NOT caught\nno-tmp caught\n")')" = catalog-broke ] || bad "31: a mutant not caught"
[ "$(variant crash 'open(os.path.join(d, "crash.jsonl"), "w").write("")')" = catalog-broke ] || bad "31: no crash result"
[ "$(variant nofull 'os.unlink(os.path.join(d, "full.jsonl"))')" = inconclusive ] || bad "31: inconclusive without full"
ok "31-ckpt.py: each outcome on runs changed to imply it"

# ------------------------------------------------------------------ 2. the decisions of 32 and 33
d32() { # d32 ROWS...: the outcome key of 32-decide.py on a CSV of these rows
  { echo "run,ssh_s,p50_full_ms,p99_full_ms,max_full_ms,p50_fsync_ms,held,pause_ms,guest_max_wait_ms,note"
    printf '%s\n' "$@"; } > "$T/32.csv"
  python3 "$KIT_ROOT/py/32-decide.py" "$T/32.csv" | cut -d'|' -f1
}
FILE="file,40,1.0,2,5,0.3,,,,"
CK="checkpoint,,,,,,held,0.01,35,"
[ "$(d32 "$FILE" "ckpt,50,1.5,3,9,0.4,,,," "py,70,3,6,20,1,,,," "$CK")" = boots-cheap ] || bad "32: boots-cheap"
[ "$(d32 "$FILE" "ckpt,50,2.5,3,9,0.4,,,," "py,70,3,6,20,1,,,," "$CK")" = boots-slow ] || bad "32: boots-slow (writes)"
[ "$(d32 "$FILE" "py,70,1.5,6,20,1,,,," "$CK")" = boots-slow ] || bad "32: boots-slow (start, py only)"
[ "$(d32 "$FILE" "ckpt,,,,,,,,,client errors: 20001" "py,,,,,,,,,refused")" = no-boot ] || bad "32: no-boot"
[ "$(d32 "$FILE" "py,,,,,,,,,unix refused" "py-tcp,45,1.2,3,9,0.4,,,," "$CK")" = boots-cheap ] || bad "32: boots over TCP"
[ "$(d32 "$FILE" "ckpt,50,1.5,3,9,0.4,,,," "py,70,3,6,20,1,,,," "checkpoint,,,,,,lost 3 of 200,0.01,35,")" = checkpoint-broke ] || bad "32: checkpoint-broke"
[ "$(d32 "$FILE" "ckpt,50,1.5,3,9,0.4,,,," "py,70,3,6,20,1,,,," "checkpoint,,,,,,did not boot,,,")" = checkpoint-broke ] || bad "32: checkpoint did not boot"
[ "$(d32 "file,,,,,,,,,no ssh" "py,70,3,6,20,1,,,,")" = inconclusive ] || bad "32: inconclusive"
ok "32-decide.py: each outcome"

d33() { # d33 WRITTEN ROWS...: target,stage,alloc_bytes rows
  local w="$1"; shift
  { echo "target,stage,alloc_bytes,size_bytes"; printf '%s\n' "$@"; } > "$T/33.csv"
  python3 "$KIT_ROOT/py/33-decide.py" "$T/33.csv" "$w" | cut -d'|' -f1
}
G=1000000000
rows() { # rows TARGET before written deleted settled remount restart (GB, "-" for none)
  local t="$1" s v i=0; shift
  for v in "$@"; do
    i=$((i + 1)); s="$(echo before written deleted settled remount restart | cut -d' ' -f$i)"
    [ "$v" = - ] || echo "$t,$s,$((v * G / 10)),0"
  done
}
# shellcheck disable=SC2046
{
[ "$(d33 $((3 * G)) $(rows main 200 230 230 201 - 201) $(rows raw 1 31 31 2 2 2))" = trim-reaches ] || bad "33: trim-reaches"
[ "$(d33 $((3 * G)) $(rows main 200 230 230 230 - 201) $(rows raw 1 31 31 31 2 2))" = trim-at-mount ] || bad "33: trim-at-mount"
[ "$(d33 $((3 * G)) $(rows main 200 230 230 230 - 230) $(rows asif 1 31 31 2 2 2))" = asif-only ] || bad "33: asif-only"
[ "$(d33 $((3 * G)) $(rows main 200 230 230 230 - 229) $(rows raw 1 31 31 31 31 31))" = no-trim ] || bad "33: no-trim"
[ "$(d33 $((3 * G)) $(rows main 200 230 230 215 - 215))" = partial ] || bad "33: partial"
[ "$(d33 $((3 * G)) $(rows main 200 201 201 201 - 201))" = inconclusive ] || bad "33: inconclusive (no growth)"
}
ok "33-decide.py: each outcome"

# ------------------------------------------------------------------ 3. the servers and lib/nbd.sh
(
  export KIT_ID=ckpt KIT_WORK="$T/w" KIT_OUT="$T/lib.jsonl" KIT_WORK_ROOT="$T/root"
  mkdir -p "$KIT_WORK" "$KIT_WORK_ROOT/bin"; : > "$KIT_OUT"
  # shellcheck disable=SC1091
  . "$KIT_ROOT/lib/common.sh"
  # shellcheck disable=SC1091
  . "$KIT_ROOT/lib/ckpt.sh"
  # shellcheck disable=SC1091
  . "$KIT_ROOT/lib/nbd.sh"
  NB="python3 -I $KIT_ROOT/py/nbdserve.py"
  head -c $((16 * 1048576)) /dev/zero > "$T/w/disk.img"
  for addr in "$T/w/n.sock" "tcp:127.0.0.1:$(nbd_free_port)"; do
    pid="$(nbd_start py "$T/w/disk.img" "$addr" "$T/w/py.log" "$T/w/c.sock")" || bad "nbd_start py on $addr"
    u="$(nbd_url "$addr")"
    case "$addr" in tcp:*) [ "${u#nbd://127.0.0.1:}" != "$u" ] || bad "nbd_url tcp: $u" ;; *) [ "$u" = "nbd+unix:///disk?socket=$addr" ] || bad "nbd_url: $u" ;; esac
    p="$($NB probe "$u")" || bad "probe $u: $p"
    printf '%s' "$p" | grep -q '"trim": true' || bad "the py server does not advertise TRIM: $p"
    $NB writer "$u" 1 "$T/w/w.log" 200 > "$T/w/w.out" || bad "writer on $u: $(cat "$T/w/w.out")"
    v="$($NB verify "$T/w/disk.img" "$T/w/w.log")" || bad "the served image does not hold the writes: $v"
    st="$(nbd_ctl "$T/w/c.sock" stats)" || bad "ctl stats"
    printf '%s' "$st" | python3 -c 'import json,sys; d=json.load(sys.stdin); sys.exit(0 if d["connections"] == 2 and d["write"] > 50 and not d["errors"] else 1)' \
      || bad "stats after a probe and a writer: $st"
    nbd_stop "$pid" || bad "nbd_stop"
    kill -0 "$pid" 2>/dev/null && bad "the server outlived nbd_stop"
    $NB probe "$u" >/dev/null 2>&1 && bad "a stopped server still answers on $u"
    echo "ok   lib/nbd.sh: py server on $addr starts, serves a probe and 1 s of writes, counts them, stops"
  done
  nbd_start ckpt "$T/w/disk.img" "tcp:127.0.0.1:1" "$T/w/x.log" >/dev/null 2>&1 && bad "the prototype's server was started on TCP"
  # A server that cannot listen is reported, not waited on forever.
  nbd_start py "$T/w/missing.img" "$T/w/m.sock" "$T/w/m.log" >/dev/null 2>&1 && bad "nbd_start succeeded on a missing image"
  printf 'nbd: client left after 2 requests\nnbd: BadRequest\n' > "$T/w/z.log"
  [ "$(nbd_errors "$T/w/z.log")" = "nbd: BadRequest" ] || bad "nbd_errors on the prototype's log"
  echo "ok   lib/nbd.sh: refuses TCP for the prototype, reports a server that dies, reads both logs' errors"

  # The fake silovm's --disk-nbd against a served disk.
  cp "$HERE/fake-silovm" "$KIT_WORK_ROOT/bin/silovm"; chmod +x "$KIT_WORK_ROOT/bin/silovm"
  "$SILOVM" install --ipsw "$KIT_ROOT/kit" --out "$T/w/b.silovm" > /dev/null 2>&1 || bad "fake install"
  pid="$(nbd_start py "$T/w/disk.img" "$T/w/n.sock" "$T/w/py.log")" || bad "nbd_start for the fake"
  "$SILOVM" run --bundle "$T/w/b.silovm" --disk-nbd "$(nbd_url "$T/w/n.sock")" --max-seconds 2 > "$T/w/run.jsonl" 2>/dev/null
  grep -q '"event": "nbd", "state": "connected"' "$T/w/run.jsonl" || bad "fake silovm --disk-nbd: $(cat "$T/w/run.jsonl")"
  nbd_stop "$pid"
  "$SILOVM" run --bundle "$T/w/b.silovm" --disk-nbd "$(nbd_url "$T/w/n.sock")" --max-seconds 2 > "$T/w/run2.jsonl" 2>/dev/null
  grep -q '"networkBlockDeviceNegotiationFailed"' "$T/w/run2.jsonl" || bad "fake silovm --disk-nbd with no server: $(cat "$T/w/run2.jsonl")"
  "$SILOVM" run --bundle "$T/w/b.silovm" --disk-nbd /not/a/url > /dev/null 2>&1 && bad "fake silovm took --disk-nbd /not/a/url"
  echo "ok   fake silovm --disk-nbd connects to a served disk, reports a missing server, refuses a path"
) || exit 1

# ------------------------------------------------------------------ 4. the analogues of 32 and 33
run_test() { # run_test ID LABEL [VAR=value...]: prints the outcome key, or skip:/error:
  local id="$1" label="$2"; shift 2
  local out="$T/r-$id-$label.jsonl" f
  f="$(ls "$KIT_ROOT"/tests/"$id"-*.sh)"
  : > "$out"; rm -rf "$T/work-$id"; mkdir -p "$T/work-$id" "$T/root/bin"
  env "$@" KIT_FAKE=1 KIT_YES=1 KIT_WORK_ROOT="$T/root" KIT_ID="$id" KIT_WORK="$T/work-$id" KIT_OUT="$out" \
    bash "$f" < /dev/null > "$T/log-$id-$label.txt" 2>&1
  python3 - "$out" <<'EOF'
import json, sys
key = None
for line in open(sys.argv[1]):
    e = json.loads(line)
    if e["kind"] == "outcome": key = e["key"]
    if e["kind"] in ("skip", "error"): key = e["kind"] + ":" + e.get("reason", "")
print(key or "none")
EOF
}
expect() { # expect ID LABEL WANT [VAR=value...]
  local id="$1" label="$2" want="$3" got; shift 3
  got="$(run_test "$id" "$label" "$@")"
  case "$got" in
    "$want") ok "test $id ($label): $got" ;;
    skip:*) echo "skip test $id ($label): ${got#skip:}" ;;
    *) tail -30 "$T/log-$id-$label.txt" >&2; bad "test $id ($label): wanted $want, got $got" ;;
  esac
}
expect 32 analogue boots-cheap NBDSERVE_COPY_DELAY=0.004
expect 32 no-cow checkpoint-broke NBDSERVE_COPY_DELAY=0.004 NBDSERVE_MUTANT=no-cow
expect 33 analogue trim-at-mount

# ------------------------------------------------------------------ 5. test 31, small
Z="${KIT_ZIG:-$(command -v zig 2>/dev/null)}"
P="${KIT_CKPT_PROTO:-$KIT_ROOT/../2026-11-24-checkpoint-engine/proto}"
if [ -n "$Z" ] && [ "$("$Z" version 2>/dev/null)" = 0.15.2 ] && [ -f "$P/build.zig" ]; then
  mkdir -p "$T/root/bin"
  cc -O2 -o "$T/root/bin/extents" "$KIT_ROOT/c/extents.c" || bad "extents.c"
  got="$(run_test 31 analogue KIT_ZIG="$Z" KIT_CKPT_PROTO="$P")"
  case "$got" in
    skip:*) echo "skip test 31: ${got#skip:}" ;;
    shared-clone-*|full-free) ok "test 31 (analogue, built with $Z): $got" ;;
    *) tail -30 "$T/log-31-analogue.txt" >&2; bad "test 31: got $got" ;;
  esac
else
  echo "skip test 31 end to end: needs Zig 0.15.2 (KIT_ZIG or PATH) and the prototype's source (KIT_CKPT_PROTO, or next to the kit)"
fi
echo "test-ckpt: all passed"
