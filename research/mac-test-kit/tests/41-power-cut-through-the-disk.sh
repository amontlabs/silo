#!/bin/bash
# id: 41
# title: A power cut under the record's writer: a macOS computer on a disk that loses its cache
# tier: vm
# time: about 40 minutes (seven starts of a computer)
# needs: the base computer of test 00 (raw disk), 40 GB free, no sudo; Zig 0.15.2 and the writer's source, as test 31 (both asked first)
# step: 1
# decides: record-file-format (the flush before an acknowledgement), record-damaged-at-start (the after-head start rule), research 2026-11-25-durability-on-a-mac, research 2026-11-25-record-head-off-the-mac
# question: When a macOS computer loses power while the record's writer runs, with its disk served by a stand-in drive that keeps writes in a cache until a flush and loses part of that cache in the cut, does every line the writer acknowledged survive and the record start again, with every flush F_FULLFSYNC, with the barrier rule, and with plain fsync?
# outcome full-holds-fsync-loses: With F_FULLFSYNC, and with the barrier rule, every acknowledged line survived every cut and the record started; with plain fsync some did not: the writer's flushes are right on APFS, and the barrier rule may be used where test 40 says it saves time.
# outcome barrier-loses: F_FULLFSYNC kept every acknowledged line, the barrier rule did not: F_BARRIERFSYNC does not order the writes the rule needs through this disk; the writer uses F_FULLFSYNC for every flush.
# outcome fsync-holds: Even plain fsync kept every acknowledged line: the computer's disk path flushed on its own (the flush counts are in the facts), so this stand-in cannot tell the flushes apart; trust #394's models and test 40 instead.
# outcome full-loses: An acknowledged line was lost even with every flush F_FULLFSYNC: F_FULLFSYNC does not reach the disk through this path, or the writer is wrong; nothing relies on the record's durability on a Mac until the cause is found (the runs are attached).
# outcome no-start: The computer did not start again after a cut: APFS itself did not survive this stand-in's power cut, which says more about the stand-in than about the writer; look at the attached logs.
# outcome inconclusive: The computer did not start from the served disk or the writer did not run; read the facts.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/ckpt.sh"
. "$KIT_ROOT/lib/silorec.sh"

MODES="${KIT_41_MODES:-full barrier fsync}"
CUTS="${KIT_41_CUTS:-2}"            # power cuts per mode
RUN_S="${KIT_41_RUN_S:-12}"         # seconds the writer runs before the cut
KEEP="${KIT_41_KEEP:-0.5}"          # share of the cached blocks that reach the disk in the cut
R="$KIT_WORK/runs"; rm -rf "$R"; mkdir -p "$R"
CSV="$R/41-cuts.csv"
echo "mode,cut,last_acked,started,verify_ok,lines,cached,kept,flushes,acks,note" > "$CSV"
CD="python3 -I $KIT_ROOT/py/cutdisk.py"

lines_of() { sed -n 's/^\([0-9]*\) lines checked.*/\1/p' | tail -1; }
last_ack() { sed -n 's/^\([0-9]*\) acked$/\1/p' "$1" | tail -1; }

decide() {
  python3 "$KIT_ROOT/py/41-decide.py" "$CSV" > "$R/record.tsv" || kit_fail "py/41-decide.py failed"
  local res="" kind a b c e
  while IFS="$(printf '\t')" read -r kind a b c e; do
    case "$kind" in M) kit_measure "$a" "$b" "$c" "$e" ;; F) kit_fact "$a" "$b" ;; O) res="$a|$b" ;; esac
  done < "$R/record.tsv"
  [ -n "$res" ] || kit_fail "no outcome from py/41-decide.py"
  kit_attach "$CSV"
  kit_outcome "${res%%|*}" "$1${res#*|}"
}

if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then
  # Linux analogue: the stand-in drive's own check (with a planted bug it must catch), then the
  # writer killed with SIGKILL as it acknowledges, which loses no cache and checks the parsing.
  $CD selfcheck > "$R/selfcheck.txt" 2>&1; kit_attach "$R/selfcheck.txt"
  [ "$(tail -1 "$R/selfcheck.txt")" = PASS ] || kit_fail "the stand-in drive's self-check failed"
  [ "$(CUTDISK_MUTANT=flush-noop $CD selfcheck 2>&1 | tail -1)" = FAIL ] || kit_fail "the self-check missed a flush that does nothing"
  if [ -n "${KIT_SILOREC:-}" ]; then SILOREC_BIN="$KIT_SILOREC"; else silorec_build || kit_skip "the writer could not be built here (see the log)"; fi
  for m in $MODES; do
    rm -rf "$KIT_WORK/r-$m"
    SILOREC_FLUSH=$m SILOREC_ACKS=1 "$SILOREC_BIN" bench "$KIT_WORK/r-$m" 100000000 1 200 5 > "$R/acks-$m.txt" 2>/dev/null &
    p=$!; sleep 2; kill -9 "$p"; wait "$p" 2>/dev/null
    la="$(last_ack "$R/acks-$m.txt")"
    st=yes; "$SILOREC_BIN" start "$KIT_WORK/r-$m" 65536 > "$R/start-$m.txt" 2>&1 || st=no
    v="$("$SILOREC_BIN" verify "$KIT_WORK/r-$m" c1 2>&1)"; vo=$([ $? = 0 ] && echo yes || echo no)
    echo "$m,1,${la:-0},$st,$vo,$(printf '%s\n' "$v" | lines_of),,,,$(grep -c acked "$R/acks-$m.txt"),SIGKILL not a power cut" >> "$CSV"
    rm -rf "$KIT_WORK/r-$m"
  done
  decide "Linux analogue, a SIGKILL and no power cut, nothing booted: "
  exit 0
fi

kit_need_mac 14
kit_need_silovm
kit_need_base_vm
kit_need_free_gb 40
kit_confirm "Test 41 starts a macOS computer about seven times, its disk served by a stand-in drive on this Mac that loses part of its cache in each of $((CUTS * $(echo $MODES | wc -w))) power cuts: about 40 minutes."
silorec_build || { kit_outcome inconclusive "the writer did not build"; exit 0; }

B="$(vm_fresh t41)" || exit 1
fmt="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("disk_format", "raw"))' "$B/config.json")"
[ "$fmt" = raw ] || kit_skip "the base computer's disk is $fmt; the stand-in drive serves a raw image"
SOCK="$KIT_WORK/n.sock"; CTL="$KIT_WORK/c.sock"
[ "${#SOCK}" -lt 100 ] || { SOCK="/tmp/silokit-41-$$.sock"; CTL="/tmp/silokit-41-$$.ctl"; }
SPID=""; WP=""
trap '[ -n "$WP" ] && kill "$WP" 2>/dev/null; vm_stop "$B" 10 >/dev/null; [ -n "$SPID" ] && kill "$SPID" 2>/dev/null; vm_destroy "$B"; rm -f "$SOCK" "$CTL"' EXIT

# First start, provisioned, from the file; the writer copied in.
# shellcheck disable=SC2046  # the options are words on purpose
vm_start "$B" $(vm_provision_args) >/dev/null
vm_ready "$B" "$VM_USER" 900 >/dev/null || { kit_attach "$B/run.log"; kit_fail "the computer did not answer SSH after its first start"; }
vm_ssh "$B" "$VM_USER" 'mkdir -p ~/sk && cat > ~/sk/silorec && chmod +x ~/sk/silorec' < "$SILOREC_BIN" || kit_fail "copy of the writer"
vm_stop "$B" 60 >/dev/null

seed=1
for m in $MODES; do
  for k in $(seq 1 "$CUTS"); do
    note=""
    rm -f "$SOCK" "$CTL"
    $CD serve --image "$B/disk.img" --unix "$SOCK" --control "$CTL" --seed "$seed" > "$R/server-$m-$k.log" 2>&1 &
    SPID=$!; seed=$((seed + 1))
    i=0; while [ ! -S "$CTL" ] && [ "$i" -lt 50 ]; do sleep 0.2; i=$((i + 1)); done
    vm_start "$B" --disk-nbd "nbd+unix:///disk?socket=$SOCK" --disk-sync full --max-seconds 1800 >/dev/null
    if ! vm_wait_ssh "$B" "$VM_USER" 600 >/dev/null; then
      kit_attach "$B/run.log"; kit_attach "$R/server-$m-$k.log"
      echo "$m,$k,,no-first,,,,,,,the computer did not start from the stand-in drive" >> "$CSV"
      decide ""; exit 0
    fi
    vm_ssh "$B" "$VM_USER" "rm -rf ~/r-$m; SILOREC_FLUSH=$m SILOREC_ACKS=1 ~/sk/silorec bench ~/r-$m 100000000 1 200 5" > "$R/acks-$m-$k.txt" 2>/dev/null &
    WP=$!
    sleep "$RUN_S"
    before="$($CD ctl "$CTL" stats)"
    cut="$($CD ctl "$CTL" cut "$KEEP")"
    "$SILOVM" ctl --bundle "$B" stop >/dev/null 2>&1   # the power goes: no shutdown
    sleep 2; vm_running "$B" && kill -9 "$(cat "$B/run.pid")" 2>/dev/null
    kill "$WP" 2>/dev/null; wait "$WP" 2>/dev/null; WP=""
    $CD ctl "$CTL" quit >/dev/null 2>&1; wait "$SPID" 2>/dev/null; SPID=""
    kit_fact "cut_${m}_${k}" "$cut"
    la="$(last_ack "$R/acks-$m-$k.txt")"
    cached="$(printf '%s' "$cut" | python3 -c 'import json,sys; print(json.load(sys.stdin)["cached"])')"
    kept="$(printf '%s' "$cut" | python3 -c 'import json,sys; print(json.load(sys.stdin)["kept"])')"
    fl="$(printf '%s' "$before" | python3 -c 'import json,sys; print(json.load(sys.stdin)["flush"])')"
    # The computer starts again from what the drive kept, as a Mac does after the power comes back.
    vm_start "$B" --max-seconds 1800 >/dev/null
    if vm_wait_ssh "$B" "$VM_USER" 600 >/dev/null; then
      st=yes; vm_ssh "$B" "$VM_USER" "~/sk/silorec start ~/r-$m 65536" > "$R/start-$m-$k.txt" 2>&1 || st=no
      v="$(vm_ssh "$B" "$VM_USER" "~/sk/silorec verify ~/r-$m c1" 2>&1)"; vo=$([ $? = 0 ] && echo yes || echo no)
      printf '%s\n' "$v" > "$R/verify-$m-$k.txt"
      echo "$m,$k,${la:-0},$st,$vo,$(printf '%s\n' "$v" | lines_of),$cached,$kept,$fl,$(grep -c acked "$R/acks-$m-$k.txt"),$note" >> "$CSV"
      vm_ssh "$B" "$VM_USER" "rm -rf ~/r-$m" >/dev/null 2>&1
      vm_stop "$B" 60 >/dev/null
    else
      kit_attach "$B/run.log"
      echo "$m,$k,${la:-0},no-boot,,,$cached,$kept,$fl,$(grep -c acked "$R/acks-$m-$k.txt"),the computer did not start after the cut" >> "$CSV"
      decide ""; exit 0
    fi
  done
done
for f in "$R"/server-*.log "$R"/start-*.txt "$R"/verify-*.txt; do [ -f "$f" ] && kit_attach "$f"; done
decide ""
