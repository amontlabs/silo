#!/bin/bash
# id: 16
# title: An idle computer for a night
# tier: long
# time: one night (8 hours by default; KIT_IDLE_HOURS sets it)
# needs: the installed computer of test 00, macOS 27 on the Mac, the Mac on power and left awake (the test holds a caffeinate assertion while it runs)
# decides: own-cost-targets and silo-is-light (the idle reference every cost target is read against), the default filter of silo changes
# question: Left idle for a night, what does a provisioned macOS computer cost the Mac in processor, memory and disk, and which paths does macOS write inside it by itself?
# outcome quiet: After the first half hour the computer is quiet: the Virtualization process uses under 5% of
#   one core (median of the quiet windows) and macOS changes fewer than 2,000 files per half hour. The
#   numbers are the idle reference of own-cost-targets; ranked.txt is the first default filter of
#   silo changes (folders changed in most windows are hidden by default, with a flag to show them).
# outcome busy: The idle computer keeps working: over 5% of a core, or over 2,000 changed files per half
#   hour after the first half hour. The ranked list says who (Spotlight, software update, logs); Silo turns
#   those off in setup, or the cost targets are read against this higher floor.
# outcome inconclusive: Fewer than three samples or no window of changed files; read samples.tsv.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"

HOURS="${KIT_IDLE_HOURS:-8}"
EVERY="${KIT_IDLE_EVERY_SECONDS:-300}"        # one sample every 5 minutes
FIND_EVERY="${KIT_IDLE_FIND_EVERY:-6}"         # the changed files every 6th sample (30 minutes)
DATA=/System/Volumes/Data                      # where every write of a macOS guest lands
if guest_local; then
  HOURS="${KIT_IDLE_HOURS:-0.003}"; EVERY="${KIT_IDLE_EVERY_SECONDS:-2}"; FIND_EVERY="${KIT_IDLE_FIND_EVERY:-2}"
  DATA="$GUEST_HOME/data"
  kit_note "Linux run: a fake computer; a writer in $DATA stands in for macOS; $HOURS h, a sample every $EVERY s"
else
  kit_need_mac 27
  kit_need_silovm; kit_need_base_vm
  kit_confirm "This starts one macOS computer and leaves it idle for $HOURS hours, sampling it every $((EVERY / 60)) minutes; it keeps the Mac awake meanwhile (caffeinate, ended with the test)."
fi

trap 'guest_cleanup; [ -n "${CAF:-}" ] && kill "$CAF" 2>/dev/null; [ -n "${WRITER:-}" ] && kill "$WRITER" 2>/dev/null' EXIT
guest_up idle
guest_local || kit_measure seconds_to_ssh "$GUEST_SSH_SECONDS" s "first start with provisioning"
if guest_local; then
  mkdir -p "$DATA/Library/Logs" "$DATA/private/var/db" "$DATA/Users/silokit/Library/Caches"
  ( while :; do date > "$DATA/Library/Logs/system.log"; date > "$DATA/private/var/db/state-$((RANDOM % 3))"; sleep 1; done ) &
  WRITER=$!
else
  caffeinate -i -w $$ & CAF=$!
fi

ch="$KIT_WORK/changes"; rm -rf "$ch"; mkdir -p "$ch"
samples="$KIT_WORK/samples.tsv"
printf 'n\tt\tsilovm_time\tvz_time\tvz_rss_kb\tvz_count\tdisk_blocks\tdf_free_kb\tguest_used_kb\tguest_mem_free_pct\tfind_s\tchanged\tdenied\n' > "$samples"

# vz_pids: the Virtualization processes on the Mac (the one that holds the guest's memory).
vz_pids() {
  if guest_local; then cat "$GB/fake.pid" 2>/dev/null; return; fi
  pgrep -f com.apple.Virtualization.VirtualMachine
}

# The guest side of a sample: used space of the Data volume, free memory, and, when asked, every
# file changed since the last marker (find -newer), as paths relative to the Data volume.
GUEST_SAMPLE='d="'"$DATA"'"; m=/var/tmp/silokit-16.marker; [ -n "$LOCALTMP" ] && m="$LOCALTMP/silokit-16.marker"
used=$(df -k "$d" | awk "NR==2 {print \$3}")
mem=$(memory_pressure -Q 2>/dev/null | awk -F": " "/free percentage/ {print \$2}" | tr -d "%")
echo "used $used"; echo "mem $mem"
if [ "$FIND" = 1 ]; then
  touch "$m.new"
  if [ -f "$m" ]; then
    t0=$(date +%s)
    find "$d" -xdev -newer "$m" \( -type f -o -type l \) 2> "$OUT.err" | sed "s#^$d##" > "$OUT"
    echo "find_s $(( $(date +%s) - t0 ))"; echo "changed $(wc -l < "$OUT" | tr -d " ")"
    echo "denied $(grep -c "not permitted" "$OUT.err")"
    chmod 644 "$OUT"
  fi
  mv "$m.new" "$m"
fi'

end="$(python3 -c "import time; print(time.time() + $HOURS * 3600)")"
n=0
while :; do
  n=$((n + 1))
  t="$(kit_now)"
  spid="$(cat "$GB/run.pid" 2>/dev/null)"
  pids="$(vz_pids | tr '\n' ' ')"
  vz_count="$(echo "$pids" | wc -w | tr -d ' ')"
  vpid="$(echo "$pids" | awk '{print $1}')"
  stime="$(ps -o time= -p "$spid" 2>/dev/null | tr -d ' ')"
  vtime=""; vrss=""
  [ -n "$vpid" ] && vtime="$(ps -o time= -p "$vpid" 2>/dev/null | tr -d ' ')" && vrss="$(ps -o rss= -p "$vpid" 2>/dev/null | tr -d ' ')"
  if guest_local; then blocks="$(du -k "$GB/disk.img" | awk '{print $1 * 2}')"; else blocks="$(stat -f %b "$GB/disk.img" 2>/dev/null)"; fi
  free="$(df -k "$GB" | awk 'NR==2 {print $4}')"
  find=0; [ $((n % FIND_EVERY)) = 1 ] || [ "$FIND_EVERY" = 1 ] && find=1
  out="/var/tmp/silokit-16-changes.txt"; lt=""
  guest_local && { out="$KIT_WORK/guest-changes.txt"; lt="$KIT_WORK"; }
  g="$(guest_sudo "FIND=$find OUT=$out LOCALTMP=$lt; $GUEST_SAMPLE" 2>/dev/null)"
  used="$(echo "$g" | awk '$1 == "used" {print $2}')"
  mem="$(echo "$g" | awk '$1 == "mem" {print $2}')"
  fs="$(echo "$g" | awk '$1 == "find_s" {print $2}')"
  changed="$(echo "$g" | awk '$1 == "changed" {print $2}')"
  denied="$(echo "$g" | awk '$1 == "denied" {print $2}')"
  if [ -n "$changed" ]; then
    if guest_local; then cp "$out" "$ch/changes-$(printf %03d "$n").txt"; else guest_get "$out" "$ch/changes-$(printf %03d "$n").txt"; fi
  fi
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$n" "$t" "$stime" "$vtime" "$vrss" "$vz_count" "$blocks" "$free" "$used" "$mem" "$fs" "$changed" "$denied" >> "$samples"
  kit_log "sample $n: VM cpu time $vtime, rss ${vrss} KB, guest used ${used} KB, changed ${changed:--}"
  vm_running "$GB" || { kit_note "the computer stopped during the night: $(vm_event "$GB" stopped)"; break; }
  python3 -c "import sys, time; sys.exit(0 if time.time() + $EVERY < $end else 1)" || break
  sleep "$EVERY"
done
kit_attach "$samples"
denied_max="$(awk -F'\t' 'NR > 1 && $13 != "" && $13 > m {m = $13} END {print m + 0}' "$samples")"
kit_measure find_denied_max "$denied_max" count "folders root over SSH could not read (privacy protection): their changes are missing from the list"

python3 "$KIT_ROOT/py/16-idle-rank.py" "$samples" "$ch" "$KIT_WORK" > "$KIT_WORK/summary.line" || kit_fail "the summary script failed"
kit_attach "$KIT_WORK/ranked.txt"
kit_attach "$KIT_WORK/summary.json"
(cd "$KIT_WORK" && tar -czf changes.tar.gz changes) && kit_attach "$KIT_WORK/changes.tar.gz"
rm -rf "$ch"

j() { python3 -c 'import json,sys; v=json.load(open(sys.argv[1])).get(sys.argv[2]); print("null" if v is None else (", ".join(v) if isinstance(v, list) else v))' "$KIT_WORK/summary.json" "$1"; }
m() { local v; v="$(j "$1")"; [ "$v" = null ] || kit_measure "$1" "$v" "$2" "${3:-}"; }
m hours h
m vz_cpu_pct_median_quiet "% of one core" "Virtualization process, windows without a find"
m vz_cpu_pct_median_all "% of one core"
m silovm_cpu_pct_median_quiet "% of one core"
m vz_rss_mb_first MB; m vz_rss_mb_last MB; m vz_rss_mb_max MB
m disk_allocated_mb_growth MB "disk.img allocated blocks (an APFS clone shares blocks: overwrites do not show here)"
m mac_free_mb_change MB "free space of the Mac's volume; other programs move it too"
m guest_used_mb_growth MB "used space of the guest's Data volume"
m changed_files_first_window files
m changed_files_per_window_median_after_first files "per $((EVERY * FIND_EVERY / 60)) minutes"
kit_fact top_folders "$(j top_folders)"
kit_fact vz_count_max "$(j vz_count_max)"

samples_n="$(j samples)"; cpu="$(j vz_cpu_pct_median_quiet)"; files="$(j changed_files_per_window_median_after_first)"
text="$samples_n samples over $(j hours) h: VM process $cpu% of a core (median, quiet windows), memory $(j vz_rss_mb_first) to $(j vz_rss_mb_last) MB (max $(j vz_rss_mb_max)), guest data +$(j guest_used_mb_growth) MB, changed files $(j changed_files_first_window) in the first window then $files per window. Top folders: $(j top_folders)."
if [ "$samples_n" -lt 3 ] || [ "$files" = null ]; then
  kit_outcome inconclusive "$text"
elif python3 -c "import sys; c='$cpu'; f=float('$files'); sys.exit(0 if (c != 'null' and float(c) < 5) and f < 2000 * $EVERY * $FIND_EVERY / 1800 else 1)"; then
  kit_outcome quiet "$text"
else
  kit_outcome busy "$text"
fi
