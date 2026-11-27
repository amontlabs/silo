#!/bin/bash
# id: 37
# title: The Mac sleeps and wakes with main running
# tier: guided
# time: about 25 minutes; you put the Mac to sleep and wake it (with --allow-sudo the kit wakes it by itself after 3 minutes)
# needs: the installed computer of test 00, macOS 27 on the Mac, network; the Mac may sleep (close nothing else that matters)
# step: 3
# decides: mac-kept-awake (does Apple's framework hold the Mac awake while a computer runs, without Silo), who-made-each-change (a sleep measured from the Mac's side as #370 writes it, with mach_continuous_time), runs-outlive-the-terminal (a run left detached keeps going across a sleep)
# question: While a computer runs, does its process hold the Mac awake by itself, and when the Mac sleeps anyway, does the computer come back whole after the wake: still running, answering SSH, a run left in tmux or screen still going, its clock right within a minute, its network working? How long was the sleep, as the Mac's clocks tell it and as the computer saw it?
# outcome wakes-whole: The computer ran on after the wake: SSH, the detached run, the network, and its clock within
#   5 s of the Mac's a minute after the wake. A sleep pauses main and costs nothing else; the record writes it
#   with author device (#370), and the helper measures it as the facts give.
# outcome clock-behind: The computer came back, but its clock was still behind the Mac's by more than 5 s a
#   minute after the wake: main needs its clock set after each wake (the helper, or sntp in main), or every
#   time in the record after a sleep is wrong.
# outcome network-lost: The computer came back, but its network did not within a minute of the wake.
# outcome run-lost: The computer stopped, stopped answering SSH, or the run left detached ended across the
#   sleep; the sentence says which. A sleep must be refused while main runs (mac-kept-awake on battery too).
# outcome no-sleep: The Mac did not sleep (the clocks saw no sleep over 30 s); the fact vm_holds_awake says
#   whether the computer's process held it awake.
# outcome inconclusive: The computer did not answer before the sleep, or a measurement is missing.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"

BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin"
SUM="$KIT_WORK/summary.env"; : > "$SUM"
put() { echo "$1=$2" >> "$SUM"; }
if guest_local; then
  kit_note "Linux run: a fake computer, and no sleep (this machine is not put to sleep): the outcome is no-sleep"
  if [ ! -x "$BIN/powerwatch" ]; then mkdir -p "$BIN"; cc -O2 -Wall -o "$BIN/powerwatch" "$KIT_ROOT/c/powerwatch.c" || kit_fail "powerwatch does not build"; fi
  put platform linux
else
  kit_need_mac 27
  kit_need_silovm; kit_need_base_vm
  [ -x "$BIN/powerwatch" ] || kit_skip "the kit's programs are not built: run ./kit build"
  kit_confirm "This starts one macOS computer, then the Mac sleeps for about 3 minutes and wakes, then the computer is checked and deleted. About 25 minutes."
  put platform mac
fi
PW=""
stop_loop() { guest_sh 'tmux kill-session -t silokit37 2>/dev/null; screen -S silokit37 -X quit 2>/dev/null; pkill -f silokit37-loop.sh; rm -f "$HOME/silokit37-loop.sh"; true' 2>/dev/null; }
trap 'stop_loop; guest_cleanup; [ -n "$PW" ] && kill "$PW" 2>/dev/null' EXIT
guest_up sleep
guest_local || kit_measure seconds_to_ssh "$GUEST_SSH_SECONDS" s "first start with provisioning"

# --- 1. Does the computer's process hold the Mac awake by itself? Read before anything else holds it.
if guest_local; then
  put vm_holds_awake unknown
else
  pmset -g assertions > "$KIT_WORK/assertions-before.txt" 2>&1
  kit_attach "$KIT_WORK/assertions-before.txt"
  # shellcheck disable=SC2046
  a="$(python3 -I "$KIT_ROOT/py/assertions.py" "$(cat "$GB/run.pid")" $(pgrep -f com.apple.Virtualization.VirtualMachine) < "$KIT_WORK/assertions-before.txt")"
  kit_fact vm_holds_awake "$(echo "$a" | head -1)"
  put vm_holds_awake "$(echo "$a" | head -1)"
  kit_fact power_source "$(pmset -g batt | head -1 | sed "s/.*'\(.*\)'.*/\1/")"
fi

# --- 2. A run left detached inside: a clock line every second, in tmux or screen when the computer
# has one (as `ssh main.keep` would), else under nohup.
KEEPER="$(guest_sh 'for t in tmux screen; do command -v $t >/dev/null 2>&1 && { echo $t; break; }; done')"
kit_fact detached_keeper "${KEEPER:-nohup}"
LOOP='rm -f "$HOME/silokit37-clock.txt"; while :; do perl -MTime::HiRes=time -e "printf qq(%.3f\n), time" >> "$HOME/silokit37-clock.txt"; sleep 1; done'
printf '%s\n' "$LOOP" > "$KIT_WORK/loop.sh"
guest_put "$KIT_WORK/loop.sh" silokit37-loop.sh
case "$KEEPER" in
  tmux) guest_sh 'tmux new-session -d -s silokit37 "sh \"$HOME/silokit37-loop.sh\""' ;;
  screen) guest_sh 'screen -dmS silokit37 sh "$HOME/silokit37-loop.sh"' ;;
  *) guest_sh '(nohup sh "$HOME/silokit37-loop.sh" > /dev/null 2>&1 &)' ;;
esac
sleep 3
alive() { guest_sh 'pgrep -f silokit37-loop.sh >/dev/null && echo yes || echo no' 2>/dev/null; }
[ "$(alive)" = yes ] || kit_fail "the detached run did not start"

# offset: the Mac's time minus the computer's, in seconds, corrected for half the round trip.
offset() {
  local t0 g t1
  t0="$(kit_now)"
  g="$(guest_sh 'perl -MTime::HiRes=time -e "printf qq(%.3f), time"' 2>/dev/null)" || return 1
  t1="$(kit_now)"
  [ -n "$g" ] || return 1
  python3 -c "print('%.2f' % (($t0 + $t1) / 2 - $g))"
}
put offset_before "$(offset)"

# --- 3. The sleep. The Mac's clocks are read every second by a process that runs across it.
"$BIN/powerwatch" 1 > "$KIT_WORK/power.txt" & PW=$!
T_SLEEP="$(kit_now)"
if guest_local; then
  sleep 3
elif [ "${KIT_SUDO:-0}" = 1 ]; then
  kit_sudo "wake the Mac by itself 3 minutes after it goes to sleep" pmset relative wake 180
  kit_log "the Mac sleeps now and wakes in 3 minutes"
  sleep 2; pmset sleepnow >/dev/null; sleep 30
else
  kit_human "Put the Mac to sleep: Apple menu > Sleep. Leave it asleep at least 3 minutes, then wake it, log in, come back here and press Enter."
fi
T_WAKE="$(kit_now)"
sleep 5
slept="$(awk '$1 == "sleep" {s += $2} END {printf "%.1f", s + 0}' "$KIT_WORK/power.txt")"
put slept_s "$slept"
kit_fact mac_sleep_s "$slept (the growth of mach_continuous_time over mach_absolute_time, read by a process running across the sleep)"
kit_attach "$KIT_WORK/power.txt"
if ! guest_local; then pmset -g log 2>/dev/null | grep -E "^$(date +%Y-%m-%d).*(Sleep|Wake) " | tail -6 > "$KIT_WORK/pmset-log.txt"; kit_attach "$KIT_WORK/pmset-log.txt"; fi

# --- 4. After the wake.
running=yes; vm_running "$GB" || running=no
put vm_running "$running"
kit_fact events_after_wake "$(python3 -c '
import json, sys
t = float(sys.argv[2]); ev = []
for line in open(sys.argv[1]):
    try: e = json.loads(line)
    except ValueError: continue
    if e.get("t", 0) >= t: ev.append(e.get("event", "?"))
print(" ".join(ev) or "none")' "$GB/run.jsonl" "$T_SLEEP" 2>/dev/null)"
ssh_ok=no; i=0
while [ "$i" -lt 24 ]; do guest_sh true 2>/dev/null && { ssh_ok=yes; break; }; sleep 5; i=$((i + 1)); done
put ssh "$ssh_ok"
if [ "$ssh_ok" = yes ]; then
  put offset_wake "$(offset)"
  put run_alive "$(alive)"
  guest_get silokit37-clock.txt "$KIT_WORK/guest-clock.txt"
  gap="$(python3 -c '
import sys
t = [float(l) for l in open(sys.argv[1]) if l.strip()]
print("%.1f" % max((b - a for a, b in zip(t, t[1:])), default=0))' "$KIT_WORK/guest-clock.txt")"
  put guest_gap_s "$gap"
  kit_fact guest_gap_s "$gap (the longest gap between two lines of the detached run's clock: the sleep as the computer saw it)"
  if guest_local; then put net ok; else
    put net "$(guest_sh 'curl -s -o /dev/null -w "%{http_code}" --max-time 20 https://www.apple.com/' 2>/dev/null | grep -q '^[23]' && echo ok || echo fail)"
  fi
  w="$(python3 -c "print(max(0, 60 - ($(kit_now) - $T_WAKE)))")"
  sleep "${w%.*}"
  put offset_60 "$(offset)"
fi
kit_attach "$SUM"
res="$(python3 -I "$KIT_ROOT/py/37-decide.py" "$SUM")"
kit_outcome "${res%%|*}" "${res#*|}"
