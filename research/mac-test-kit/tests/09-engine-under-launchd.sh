#!/bin/bash
# id: 09
# title: Start a computer from launchd: user agent, system daemon, nobody logged in
# tier: guided
# time: about 30 minutes, plus 10 minutes logged out if you agree
# needs: the installed computer of test 00, macOS 27 on the Mac, --allow-sudo for the daemon half (it loads one job in launchd's system domain and removes it)
# step: 2
# decides: where the engine runs; engine-matches-wanted (does main wait for your login after the Mac restarts)
# question: Can a program started by launchd, as a user agent and as a system daemon (with you logged in, then with nobody logged in), start a macOS computer that answers SSH?
# outcome agent-and-daemon: A user agent and a system daemon both started a computer that answered SSH
#   while you were logged in. The engine may be a launchd job; whether it may run with nobody logged in
#   is the logged-out half (run the test again after it, or it was not run).
# outcome daemon-refused: The user agent's computer started, the system daemon's did not (the facts
#   hold the error): Apple's "Virtualization is not daemon safe" holds; the engine runs in your login
#   session (a user agent or the app), and after a restart main waits for your login.
# outcome agent-refused: Even the user agent could not start a computer: the engine must be a program
#   started from your session (an app or a login item), not a launchd job; read the facts.
# outcome logged-out-works: With nobody logged in to the Mac, the daemon started a computer that
#   answered SSH: main could come back after a restart before anyone logs in (FileVault aside).
#   engine-matches-wanted need not wait for your login on a Mac without FileVault.
# outcome logged-out-refused: With nobody logged in, the daemon's computer did not start or did not
#   answer SSH (the facts say which): main comes back only once you log in; engine-matches-wanted
#   should say so.
# outcome waiting-for-logout: The logged-out half is armed. Log out, wait ten minutes at the login
#   window, log back in and run "./kit run 09 --allow-sudo" again: that run reads the result.
# outcome sudo-not-allowed: Only the user agent half ran (no --allow-sudo); the facts hold its result.
# outcome inconclusive: Something else went wrong; read the facts and the attached logs.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"

AGENT=com.silo.kit.test09.agent
DAEMON=com.silo.kit.test09.daemon
DDIR=/private/tmp/silokit-09               # the daemon's plist: launchd wants it owned by root
ENGINE="$KIT_WORK/engine.sh"
ARMED="$KIT_WORK/armed"
B="$KIT_WORK/launchd.silovm"

# The program launchd starts: it starts the computer, waits for "started", then for its SSH port,
# and writes what it saw into a result file. In after-logout mode it first waits until nobody is
# logged in (the console belongs to root at the login window), and stops the computer itself.
write_engine() {
  cat > "$ENGINE" <<'EOF'
#!/bin/sh
mode="$1"; b="$2"; silovm="$3"; deadline="$4"; res="$5"; port="${6:-22}"
now() { date +%s; }
console() { if [ "$(uname)" = Darwin ]; then stat -f %Su /dev/console; else echo "${FAKE_CONSOLE_USER:-nobody-here}"; fi; }
[ "$(now)" -lt "$deadline" ] || exit 0
if [ "$mode" = after-logout ]; then
  while [ "$(console)" != root ]; do
    [ "$(now)" -lt "$deadline" ] || { echo "result=no-logout-seen" > "$res"; exit 0; }
    sleep 5
  done
  sleep 30
fi
{ echo "mode=$mode"; echo "uid=$(id -u)"; echo "console_user=$(console)"; echo "t_begin=$(now)"; } > "$res"
"$silovm" run --bundle "$b" --max-seconds 900 > "$b/run.jsonl" 2> "$b/run.log" &
pid=$!; echo "$pid" > "$b/run.pid"
i=0; started=no
while [ "$i" -lt 90 ]; do
  grep -q '"started"' "$b/run.jsonl" 2>/dev/null && { started=yes; break; }
  kill -0 "$pid" 2>/dev/null || break
  sleep 1; i=$((i + 1))
done
echo "started=$started" >> "$res"
[ "$started" = yes ] || echo "stopped_event=$(grep '"stopped"' "$b/run.jsonl" | tail -1)" >> "$res"
ssh=no
if [ "$started" = yes ]; then
  ip="$("$silovm" ip --bundle "$b" --wait 180 2>/dev/null)"
  echo "ip=$ip" >> "$res"
  j=0
  while [ "$j" -lt 100 ] && [ -n "$ip" ]; do
    if [ "$(uname)" = Darwin ]; then nc -z -G 3 "$ip" "$port" >/dev/null 2>&1 && { ssh=yes; break; }
    else nc -z -w 3 "$ip" "$port" >/dev/null 2>&1 && { ssh=yes; break; }; fi
    sleep 3; j=$((j + 1))
  done
fi
echo "ssh_port=$ssh" >> "$res"
echo "t_ssh=$(now)" >> "$res"
if [ "$mode" = after-logout ]; then
  echo "console_user_after=$(console)" >> "$res"
  "$silovm" ctl --bundle "$b" request-stop >/dev/null 2>&1
  k=0; while kill -0 "$pid" 2>/dev/null && [ "$k" -lt 60 ]; do sleep 1; k=$((k + 1)); done
  kill -0 "$pid" 2>/dev/null && "$silovm" ctl --bundle "$b" stop >/dev/null 2>&1
fi
echo "done=1" >> "$res"
# launchd ends a job's other processes when its program exits: stay until the computer stops.
wait "$pid"
EOF
  chmod 755 "$ENGINE"
}

# write_plist PATH LABEL MODE RESULT [USERNAME]: a launchd job that runs the engine once.
write_plist() {
  python3 - "$@" "$ENGINE" "$B" "$SILOVM" "$(( $(date +%s) + 2700 ))" <<'EOF'
import plistlib, sys
path, label, mode, res = sys.argv[1:5]
rest = sys.argv[5:]
user = rest[0] if len(rest) == 5 else None
engine, bundle, silovm, deadline = rest[-4:]
job = {
    "Label": label,
    "ProgramArguments": ["/bin/sh", engine, mode, bundle, silovm, deadline, res],
    "RunAtLoad": True,
    "StandardOutPath": res + ".out",
    "StandardErrorPath": res + ".err",
}
if user:
    job["UserName"] = user
with open(path, "wb") as f:
    plistlib.dump(job, f)
EOF
}

# wait_result FILE SECONDS: wait until the engine wrote its ssh_port line.
wait_result() {
  local n=0
  while [ "$n" -lt "$2" ]; do
    grep -q '^done=1' "$1" 2>/dev/null && return 0
    grep -q '^ssh_port=' "$1" 2>/dev/null && return 0
    grep -q '^result=' "$1" 2>/dev/null && return 0
    sleep 2; n=$((n + 2))
  done
  return 1
}
val() { sed -n "s/^$2=//p" "$1" 2>/dev/null | tail -1; }

# record HALF RESULT: facts of one half; prints yes when its computer answered SSH.
record() {
  [ -f "$2" ] && kit_attach "$2"
  [ -f "$B/run.log" ] && cp "$B/run.log" "$KIT_WORK/$1-run.log" && kit_attach "$KIT_WORK/$1-run.log"
  kit_fact "$1_started" "$(val "$2" started) $(val "$2" stopped_event | cut -c1-200)"
  kit_fact "$1_ssh_port" "$(val "$2" ssh_port)"
  kit_fact "$1_console_user" "$(val "$2" console_user) uid $(val "$2" uid)"
  if [ "$(val "$2" ssh_port)" = yes ]; then echo yes; else echo no; fi
}

boot_out_daemon() {
  [ "${KIT_SUDO:-0}" = 1 ] || return 0
  kit_sudo "remove the test's launchd job from the system domain" /bin/sh -c "launchctl bootout system/$DAEMON 2>/dev/null; rm -rf $DDIR; true"
}

cleanup() {
  if ! guest_local; then
    launchctl bootout "gui/$(id -u)/$AGENT" 2>/dev/null
    [ -f "$ARMED" ] || boot_out_daemon
  fi
  [ -f "$ARMED" ] || guest_cleanup
}

# --- Linux: run the engine as launchd would, with the fake silovm; check its plist parses.
if guest_local; then
  kit_note "Linux run: no launchd; the engine runs directly with the fake silovm, and the plist is parsed back"
  rm -rf "$B"; "$SILOVM" clone --from "$KIT_BASE_VM" --to "$B"; guest_track "$B"
  trap guest_cleanup EXIT
  write_engine
  write_plist "$KIT_WORK/$AGENT.plist" "$AGENT" now "$KIT_WORK/agent.result"
  python3 -c 'import plistlib,sys; j=plistlib.load(open(sys.argv[1],"rb")); assert j["ProgramArguments"][2]=="now", j' "$KIT_WORK/$AGENT.plist" || kit_fail "the plist does not parse back"
  # A listener stands in for the computer's SSH port.
  nc -l 127.0.0.1 18022 >/dev/null 2>&1 & lis=$!
  /bin/sh "$ENGINE" now "$B" "$SILOVM" "$(( $(date +%s) + 600 ))" "$KIT_WORK/agent.result" 18022 & eng=$!
  wait_result "$KIT_WORK/agent.result" 120 || kit_fail "the engine wrote no result"
  "$SILOVM" ctl --bundle "$B" request-stop >/dev/null 2>&1
  wait "$eng"; kill "$lis" 2>/dev/null
  a="$(record agent "$KIT_WORK/agent.result")"
  kit_fact linux_engine_started "$(val "$KIT_WORK/agent.result" started)"
  kit_outcome sudo-not-allowed "Linux run: the engine started the fake computer (started=$(val "$KIT_WORK/agent.result" started)) and reached its stand-in SSH port: $a."
  exit 0
fi

kit_need_mac 27
kit_need_silovm; kit_need_base_vm
trap cleanup EXIT

# --- Second run, after the logged-out half.
if [ -f "$ARMED" ]; then
  kit_note "reading the logged-out half armed at $(cat "$ARMED")"
  res="$KIT_WORK/daemon-logout.result"
  wait_result "$res" 5 || kit_note "the logged-out half wrote no result (did you log out for long enough?)"
  r="$(record logged_out "$res")"
  [ "${KIT_SUDO:-0}" = 1 ] || kit_skip "the logged-out half's job is still loaded: run ./kit run 09 --allow-sudo to read and remove it"
  rm -f "$ARMED"
  boot_out_daemon
  guest_track "$B"
  if [ "$(val "$res" result)" = no-logout-seen ]; then
    kit_outcome inconclusive "The daemon waited 45 minutes and never saw the login window (nobody logged out in time)."
  elif [ "$r" = yes ]; then
    kit_outcome logged-out-works "With the console at the login window ($(val "$res" console_user), after: $(val "$res" console_user_after)), the daemon's computer started and its SSH port answered."
  else
    kit_outcome logged-out-refused "With nobody logged in, started=$(val "$res" started), SSH port=$(val "$res" ssh_port). $(val "$res" stopped_event | cut -c1-200)"
  fi
  exit 0
fi

kit_confirm "This provisions one macOS computer, then starts it twice from launchd: once as a job of your login session (no sudo), once as a system daemon running as you (with --allow-sudo: one job under $DDIR, removed at the end). About 30 minutes."
write_engine

# A provisioned computer with the kit's key, started once from this Terminal, then stopped.
rm -rf "$B"
guest_up launchd
s="$(vm_stop "$GB" 60)"; kit_fact provisioned_stop "$s"

# --- 1. A user agent, in launchd's domain of your login session.
plist="$KIT_WORK/$AGENT.plist"; res="$KIT_WORK/agent.result"; rm -f "$res"
write_plist "$plist" "$AGENT" now "$res"
launchctl bootstrap "gui/$(id -u)" "$plist" 2> "$KIT_WORK/agent-bootstrap.txt" || kit_note "launchctl bootstrap gui failed: $(head -1 "$KIT_WORK/agent-bootstrap.txt")"
wait_result "$res" 600 || kit_note "the user agent wrote no SSH result within 10 minutes"
agent_ok="$(record agent "$res")"
if [ "$agent_ok" = yes ]; then
  kit_fact agent_ssh_login "$(guest_sh 'sw_vers -productVersion' 2>&1 | head -1)"
fi
vm_stop "$B" 60 >/dev/null
launchctl bootout "gui/$(id -u)/$AGENT" 2>/dev/null

# --- FileVault: a question only, never a restart.
kit_fact mac_filevault "$(fdesetup status 2>&1 | head -1)"
fv="$(kit_ask_yn "When your Mac starts, does the FileVault password screen log you straight in (one password, no second login screen)?")"
kit_fact filevault_logs_in_directly "$fv"

if [ "${KIT_SUDO:-0}" != 1 ]; then
  if [ "$agent_ok" = yes ]; then
    kit_outcome sudo-not-allowed "The user agent's computer answered SSH; the daemon half needs --allow-sudo."
  else
    kit_outcome agent-refused "The user agent's computer: started=$(val "$res" started), SSH port=$(val "$res" ssh_port); the daemon half needs --allow-sudo."
  fi
  exit 0
fi

# --- 2. A system daemon running as you, with you logged in.
dplist="$KIT_WORK/$DAEMON.plist"; dres="$KIT_WORK/daemon.result"; rm -f "$dres"
write_plist "$dplist" "$DAEMON" now "$dres" "$(id -un)"
# The redirection is inside the command: kit_sudo's own line, which says why, stays on the screen.
kit_sudo "load a test job into launchd's system domain (owned by root under $DDIR, removed at the end)" \
  /bin/sh -c "install -d -o root -g wheel -m 755 $DDIR && install -o root -g wheel -m 644 '$dplist' $DDIR/$DAEMON.plist && launchctl bootstrap system $DDIR/$DAEMON.plist 2> '$KIT_WORK/daemon-bootstrap.txt'" ||
  kit_note "launchctl bootstrap system failed: $(head -2 "$KIT_WORK/daemon-bootstrap.txt" | tr '\n' ' ')"
wait_result "$dres" 600 || kit_note "the daemon wrote no SSH result within 10 minutes"
daemon_ok="$(record daemon "$dres")"
vm_stop "$B" 60 >/dev/null
boot_out_daemon

# --- 3. The logged-out half, if you agree to log out.
logout_armed=no
if [ "$daemon_ok" = yes ] || [ "$agent_ok" = yes ]; then
  agree="$(kit_ask_yn "Can you log out of your Mac now for about 10 minutes? (The kit arms a daemon that starts the computer once nobody is logged in; you run the test again afterwards.)")"
  if [ "$agree" = yes ]; then
    lres="$KIT_WORK/daemon-logout.result"; rm -f "$lres"
    write_plist "$dplist" "$DAEMON" after-logout "$lres" "$(id -un)"
    if kit_sudo "load the logged-out half's job (it starts the computer only while nobody is logged in, and does nothing after 45 minutes)" \
      /bin/sh -c "install -d -o root -g wheel -m 755 $DDIR && install -o root -g wheel -m 644 '$dplist' $DDIR/$DAEMON.plist && launchctl bootstrap system $DDIR/$DAEMON.plist"; then
      date > "$ARMED"; logout_armed=yes
      kit_stop_run "test 09 asked you to log out; run the other tests after ./kit run 09 --allow-sudo"
      kit_human "Now:
  1. Save your work. Apple menu > Log Out $(id -un). Untick \"Reopen windows when logging back in\".
  2. Stay at the login window for 10 minutes. Do not log in, and do not restart.
  3. Log back in, open Terminal, and run:  cd '$KIT_ROOT' && ./kit run 09 --allow-sudo
     That run reads the result and removes the job. (If you never come back, the job does nothing
     after 45 minutes; to remove it by hand: sudo launchctl bootout system/$DAEMON; sudo rm -rf $DDIR)
Press Enter here before you log out, so this run ends cleanly."
    fi
  fi
fi

if [ "$logout_armed" = yes ]; then
  kit_outcome waiting-for-logout "User agent SSH: $agent_ok; daemon (logged in) SSH: $daemon_ok. The logged-out half is armed."
elif [ "$agent_ok" = yes ] && [ "$daemon_ok" = yes ]; then
  kit_outcome agent-and-daemon "Both launchd jobs started a computer that answered SSH while you were logged in; the logged-out half was not run."
elif [ "$agent_ok" = yes ]; then
  kit_outcome daemon-refused "The user agent's computer answered SSH; the daemon's: started=$(val "$dres" started), SSH port=$(val "$dres" ssh_port). $(val "$dres" stopped_event | cut -c1-200)"
elif [ "$daemon_ok" = yes ]; then
  kit_outcome inconclusive "The daemon's computer answered SSH but the user agent's did not; read the facts."
else
  kit_outcome agent-refused "Neither job's computer answered SSH. Agent: started=$(val "$res" started); daemon: started=$(val "$dres" started)."
fi
