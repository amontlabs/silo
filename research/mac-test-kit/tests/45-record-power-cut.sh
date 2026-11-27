#!/bin/bash
# id: 45
# title: What a disk keeps of the record when its power is cut: F_FULLFSYNC against fsync
# tier: guided
# time: about 10 minutes per way of forcing a write (two by default); you pull a USB disk's cable while the kit writes, then plug it back
# needs: an external USB disk you can lose the contents of a folder on (its own power from the cable, so pulling the cable cuts it), named in KIT_45_VOLUME or the only external volume mounted; you at the keyboard; no sudo, no VM
# step: 5
# decides: record-damaged-at-start (#386: how often the recorder will start on a damaged line, other than an unfinished last one), record-speed-cost and engine-saved-state (whether F_FULLFSYNC, which test 06 prices, is what keeps an acknowledged line)
# question: A writer appends numbered lines with a CRC to a file on a USB disk, forcing each out (F_FULLFSYNC, or plain fsync) and noting each line it would call safe on the Mac's own disk. The disk loses power mid-write. Once it is back: is every line acknowledged before the cut there and whole? Is any line damaged with whole lines after it (damage in the middle, which #386's recorder must handle on start), or only an unfinished end?
# outcome full-keeps-acked: With F_FULLFSYNC every acknowledged line came back whole and damage, if any, was only after the last of them:
#   the record's writer can acknowledge a line once F_FULLFSYNC returns; the facts say what plain fsync kept on the same disk.
# outcome full-loses-acked: With F_FULLFSYNC, a line acknowledged before the cut is missing or damaged: this disk lies about its cache, and
#   no writer on it can promise a line; Silo keeps the record on the Mac's internal disk only, and the facts say whether fsync did worse.
# outcome inconclusive: No external volume, the cable was not pulled while lines were written, or the volume did not come back.
. "$KIT_ROOT/lib/common.sh"

W="$KIT_WORK"
PW="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin/plugwriter"
[ -x "$PW" ] || kit_skip "plugwriter is not built: run ./kit build"
MODES="${KIT_45_MODES:-full fsync}"
LINUX=0
if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then LINUX=1; fi

VOL="${KIT_45_VOLUME:-}"
if [ -z "$VOL" ] && [ "$LINUX" = 0 ]; then
  ext="$(for v in /Volumes/*; do
    [ -d "$v" ] || continue
    diskutil info "$v" 2>/dev/null | grep -Eq 'Device Location: +External|Protocol: +USB' && echo "$v"
  done)"
  [ "$(echo "$ext" | grep -c .)" = 1 ] && VOL="$ext"
fi
[ -n "$VOL" ] && [ -d "$VOL" ] || kit_skip "needs one external USB disk: plug one in, or name it with KIT_45_VOLUME=/Volumes/NAME"
[ "$LINUX" = 1 ] || kit_fact disk "$(diskutil info "$VOL" 2>/dev/null | grep -E 'Device / Media Name|Protocol|File System Personality|Solid State' | sed 's/  */ /g' | tr '\n' ';')"
kit_confirm "Write files into $VOL/silokit-45 (made and deleted by the test). You will pull that disk's cable while the kit writes, $(echo "$MODES" | wc -w | tr -d ' ') times; macOS will say the disk was not ejected properly."
DIR="$VOL/silokit-45"
mkdir -p "$DIR" || kit_skip "cannot write to $VOL"
cleanup() { rm -rf "$DIR" 2>/dev/null; }
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

: > "$W/cuts.txt"
for mode in $MODES; do
  f="$DIR/$mode.rec"
  "$PW" write "$f" "$mode" > "$W/$mode.acks" 2> "$W/$mode.err" &
  wp=$!
  sleep "${KIT_45_WRITE_SECONDS:-8}"
  if [ "$LINUX" = 1 ] && [ -n "${KIT_45_CUT:-}" ]; then
    # The self-test's cut: the writer is killed at once (no power cut, no cache lost).
    sh -c "$KIT_45_CUT"
  else
    kit_human "Pull the cable of $VOL now, while the kit writes with $mode (do not eject it). Then press Enter."
  fi
  i=0; while kill -0 "$wp" 2>/dev/null && [ "$i" -lt 30 ]; do sleep 1; i=$((i + 1)); done
  kill -9 "$wp" 2>/dev/null; wait "$wp" 2>/dev/null
  last="$(tail -1 "$W/$mode.acks" | awk '{print $2}')"
  [ -n "$last" ] || { echo "$mode no-acks" >> "$W/cuts.txt"; continue; }
  if [ "$LINUX" = 0 ] || [ -z "${KIT_45_CUT:-}" ]; then
    kit_human "Plug $VOL back in and wait until it shows in the Finder. Then press Enter."
  fi
  i=0; while [ ! -f "$f" ] && [ "$i" -lt 60 ]; do sleep 1; i=$((i + 1)); done
  [ -f "$f" ] || { echo "$mode not-back" >> "$W/cuts.txt"; continue; }
  r="$("$PW" check "$f" "$last")"
  echo "$mode $r" >> "$W/cuts.txt"
  kit_fact "cut.$mode" "$r"
  kit_fact "writer_error.$mode" "$(tail -1 "$W/$mode.err")"
  rm -f "$f"
done
kit_attach "$W/cuts.txt"

res="$(python3 - "$W/cuts.txt" <<'EOF'
import json, sys
rows = {}
for line in open(sys.argv[1]):
    mode, _, rest = line.strip().partition(" ")
    try: rows[mode] = json.loads(rest)
    except ValueError: rows[mode] = None
full = rows.get("full")
def say(m, r):
    return "%s: %d of %d acknowledged lines lost, %d whole lines after damage" % (m, r["acked_lost"], r["last_ack"] + 1, r["whole_after_damage"]) if r else "%s: no reading" % m
txt = "; ".join(say(m, r) for m, r in rows.items())
if not full or "acked_lost" not in full:
    print("inconclusive\tno reading with F_FULLFSYNC (" + txt + ")")
elif full["acked_lost"] == 0 and full["whole_after_damage"] == 0:
    print("full-keeps-acked\t" + txt)
else:
    print("full-loses-acked\t" + txt)
EOF
)"
where="$([ "$LINUX" = 1 ] && echo "Linux self-test, a killed writer and no power cut: " || echo "")"
kit_outcome "${res%%	*}" "$where${res#*	}"
