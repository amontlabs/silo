#!/bin/bash
# id: 50
# title: What Silo's own writers wait for on APFS: an acknowledgement with F_FULLFSYNC, F_BARRIERFSYNC or fsync
# tier: long
# time: about 15 minutes (18 runs of the record's writer and 4 of SQLite, quiet and while another program writes)
# needs: 2 GB free, no VM, no sudo; Zig 0.15.2 and the writer's source, as test 31 (both asked first)
# step: 1
# decides: record-file-format (the flush before an acknowledgement), record-damaged-at-start (whether the barrier rule of research 2026-11-25-record-head-off-the-mac is worth it), what-a-budget-promises (the spend journal's two flushes per model call), pooled-budget-ledger (the team ledger's commit on a Mac), research 2026-11-25-durability-on-a-mac
# question: On this Mac's APFS, what does the record's writer wait for before it acknowledges a line, when every flush is F_FULLFSYNC, when only the flush before an acknowledgement is (the rest F_BARRIERFSYNC), and with plain fsync (which keeps nothing through a power cut), alone and while another program writes; and what does a SQLite commit cost with fullfsync?
# outcome full-affordable: With every flush F_FULLFSYNC, an acknowledgement waits 50 ms or less at the 99th percentile even while another program writes, new blobs included: Silo's writers flush that way everywhere, the spend journal's two flushes per model call cost nothing a person sees, and no barrier rule is needed.
# outcome barrier-for-blobs: Each acknowledgement alone is 50 ms or less, but a line with a new blob passes it with every flush F_FULLFSYNC and not with the barrier rule: the writer keeps F_FULLFSYNC before each acknowledgement and F_BARRIERFSYNC for blobs and folders, as #394's mac-barrier model, and test 51 must show the barrier holds.
# outcome group-lines: An acknowledgement with F_FULLFSYNC passes 50 ms while another program writes, and grouping 16 lines per flush brings the rate back over 1,000 lines a second: the writer groups what arrives while a flush runs (one flush acknowledges them all), as the recorder's batch already does.
# outcome too-slow: Even grouped by 16, F_FULLFSYNC leaves the writer under 1,000 lines a second while another program writes: the record's flush needs another design (a longer batch, or acknowledging after the flush is only issued) before the first build relies on it.
# outcome inconclusive: The writer did not build or a run failed; look at the attached logs.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/ckpt.sh"
. "$KIT_ROOT/lib/silorec.sh"

R="$KIT_WORK/runs"; D="$KIT_WORK/d"; BG=""
LINES_FULL="${KIT_40_LINES:-400}"
cleanup() { [ -n "$BG" ] && { kill "$BG" 2>/dev/null; wait "$BG" 2>/dev/null; }; rm -rf "$D"; }
trap cleanup EXIT
rm -rf "$R" "$D"; mkdir -p "$R" "$D"

if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then
  kit_note "Linux analogue: every mode is fsync and fdatasync there; numbers say nothing of a Mac"
  LINES_FULL="${KIT_40_LINES:-200}"
  if [ -n "${KIT_SILOREC:-}" ]; then SILOREC_BIN="$KIT_SILOREC"; else silorec_build || kit_skip "the writer could not be built here (see the log)"; fi
else
  kit_need_mac 14
  kit_need_free_gb 2
  kit_fact power "$(pmset -g batt | head -1 | sed "s/.*'\(.*\)'.*/\1/")"
  kit_fact filevault "$(fdesetup status | head -1)"
  kit_confirm "Test 50 builds the record's writer and times its flushes on this Mac's disk, quiet and while another program writes: about 15 minutes."
  silorec_build || { kit_outcome inconclusive "the writer did not build"; exit 0; }
fi

# busy_start: another program writing 8 MiB at a time and flushing it, as a build or a copy does.
busy_start() {
  python3 - "$D/busy" <<'PY' &
import os, sys
fd = os.open(sys.argv[1], os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
buf = os.urandom(8 << 20)
while True:
    for i in range(32):
        os.pwrite(fd, buf, i * len(buf))
    os.fsync(fd)
PY
  BG=$!
  sleep 2
}
busy_stop() { kill "$BG" 2>/dev/null; wait "$BG" 2>/dev/null; BG=""; rm -f "$D/busy"; }

run() { # run LOAD MODE PER_FLUSH BLOB_EVERY
  local out
  out="$(SILOREC_FLUSH="$2" "$SILOREC_BIN" bench "$D/rec" "$LINES_FULL" "$3" 200 "$4" 2> "$R/err.txt")" || {
    kit_attach "$R/err.txt"; kit_outcome inconclusive "silorec bench $2 $3 $4 failed: $(tail -1 "$R/err.txt")"; exit 0; }
  printf '%s\n' "$out" | python3 -c 'import json,sys; d=json.loads(sys.stdin.read()); d["load"]=sys.argv[1]; print(json.dumps(d))' "$1" >> "$R/bench.jsonl"
  kit_log "$1 $2 per_flush=$3 blob_every=$4: $out"
}

sqlite_run() { # sqlite_run LOAD FULLFSYNC: 300 commits of one row, WAL, synchronous FULL
  python3 - "$D/l.db" "$1" "$2" >> "$R/bench.jsonl" <<'PY'
import json, os, sqlite3, sys, time
p, load, full = sys.argv[1], sys.argv[2], sys.argv[3] == "1"
for s in ("", "-wal", "-shm"):
    try: os.unlink(p + s)
    except FileNotFoundError: pass
db = sqlite3.connect(p, isolation_level=None)
db.execute("PRAGMA journal_mode=WAL"); db.execute("PRAGMA synchronous=FULL")
db.execute("PRAGMA fullfsync=%d" % full); db.execute("PRAGMA checkpoint_fullfsync=%d" % full)
db.execute("CREATE TABLE g (id INTEGER PRIMARY KEY, mac TEXT, amount REAL)")
lat = []
for i in range(300):
    t = time.perf_counter(); db.execute("INSERT INTO g (mac, amount) VALUES (?, ?)", ("m%d" % (i % 12), 2.0)); lat.append(time.perf_counter() - t)
lat.sort()
print(json.dumps({"sqlite": True, "load": load, "fullfsync": full, "commits": len(lat), "p50_ms": round(lat[len(lat) // 2] * 1000, 3), "p99_ms": round(lat[int(len(lat) * 0.99)] * 1000, 3), "version": sqlite3.sqlite_version}))
PY
}

for load in quiet busy; do
  [ "$load" = busy ] && busy_start
  for m in full barrier fsync; do
    run "$load" "$m" 1 0
    run "$load" "$m" 1 5
    run "$load" "$m" 16 0
  done
  sqlite_run "$load" 1; sqlite_run "$load" 0
  [ "$load" = busy ] && busy_stop
done
kit_attach "$R/bench.jsonl"

python3 "$KIT_ROOT/py/50-decide.py" "$R/bench.jsonl" > "$R/record.tsv" || kit_fail "py/50-decide.py failed on the runs"
res=""
while IFS="$(printf '\t')" read -r kind a b c e; do
  case "$kind" in
    M) kit_measure "$a" "$b" "$c" "$e" ;;
    F) kit_fact "$a" "$b" ;;
    O) res="$a|$b" ;;
  esac
done < "$R/record.tsv"
[ -n "$res" ] || kit_fail "no outcome from py/50-decide.py"
if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then kit_outcome "${res%%|*}" "Linux analogue: ${res#*|}"; else kit_outcome "${res%%|*}" "${res#*|}"; fi
