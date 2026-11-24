#!/bin/bash
# id: 22
# title: Endpoint Security events inside a computer, from an SSH session
# tier: guided
# time: about 20 minutes, with you at the computer's window for one switch (about 2 minutes)
# needs: the installed computer of test 00, macOS 27 on the Mac
# decides: denied-tool-call-mechanism and processes-in-the-record for a macOS computer (second build): can they use Endpoint Security inside it
# question: Inside a macOS computer, does eslogger (Endpoint Security) run as root from an SSH session and see an agent-like command's process starts and file writes, and if not, which grant makes it work?
# outcome es-over-ssh-works: eslogger ran as root over SSH with no grant and saw the command's exec and
#   write events: a recorder inside a macOS computer can use Endpoint Security, started over SSH.
# outcome es-needs-remote-fda: eslogger was refused (not permitted) until "Allow full disk access for
#   remote users" was turned on under Remote Login, then saw the events: Silo turns that switch on in
#   setup (a person's click, or a profile; it cannot be scripted from inside without a person), or
#   its recorder runs as a launchd daemon with its own Full Disk Access.
# outcome es-refused: eslogger was refused even with the switch on: Endpoint Security inside a
#   computer needs a signed, entitled program with its own grant (a system extension); the facts hold
#   the errors.
# outcome inconclusive: eslogger ran but saw none of the command's events, or the switch was not
#   turned on; read the attached logs.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"

ES_EVENTS="exec open write create"
MARK=silokit-es-probe

# es_count FILE: lines of eslogger's JSON output that name the probe's folder, and exec lines of
# the probe's commands.
es_count() { grep -c "$MARK" "$1" 2>/dev/null || true; }
es_execs() { grep -c '/usr/bin/touch' "$1" 2>/dev/null || true; }
es_refused() { grep -qiE 'not permitted|NOT_PERMITTED|full disk access|TCC|not authorized' "$1"; }

if guest_local; then
  # Linux: no Endpoint Security. Check the parsers on a canned eslogger line and a canned refusal.
  kit_note "Linux run: the parsers only, on canned eslogger output"
  printf '%s\n' '{"event":{"exec":{"target":{"executable":{"path":"/usr/bin/touch"}},"args":["touch","/tmp/silokit-es-probe/a"]}}}' > "$KIT_WORK/es.json"
  echo 'Failed to create ES client: ES_NEW_CLIENT_RESULT_ERR_NOT_PERMITTED' > "$KIT_WORK/es.err"
  n="$(es_count "$KIT_WORK/es.json")"; x="$(es_execs "$KIT_WORK/es.json")"
  es_refused "$KIT_WORK/es.err" && r=yes || r=no
  kit_measure linux_parsers_ok "$( [ "$n" = 1 ] && [ "$x" = 1 ] && [ "$r" = yes ] && echo 1 || echo 0)" bool
  kit_outcome inconclusive "Linux run: parsers checked (probe lines $n, exec lines $x, refusal seen $r); nothing about Endpoint Security."
  exit 0
fi

kit_need_mac 27
kit_need_silovm; kit_need_base_vm
kit_confirm "This starts one macOS computer with its screen in a window on your Mac (deleted at the end) and may ask you to turn on one switch inside it. About 20 minutes."
trap guest_cleanup EXIT
guest_up es --gui
kit_measure seconds_to_ssh "$GUEST_SSH_SECONDS" s "first start with provisioning"
kit_fact eslogger_present "$(guest_sh 'ls -l /usr/bin/eslogger 2>&1 | head -1')"
kit_fact remote_fda_before "$(guest_sh "ls ~/Documents >/dev/null 2>&1 && echo documents-readable || echo documents-refused")"

# try LABEL: eslogger as root (its own SSH session), then the probe as the user (another SSH
# session, as an agent's would be), then stop eslogger and bring its output back.
try() {
  local l="$1"
  guest_sudo "rm -f /var/tmp/es-$l.json /var/tmp/es-$l.err; nohup /usr/bin/eslogger $ES_EVENTS > /var/tmp/es-$l.json 2> /var/tmp/es-$l.err < /dev/null & echo \$! > /var/tmp/es-$l.pid; sleep 3; kill -0 \$(cat /var/tmp/es-$l.pid) 2>/dev/null && echo running || echo exited" > "$KIT_WORK/es-$l.state" 2>&1
  guest_sh "mkdir -p /tmp/$MARK && /usr/bin/touch /tmp/$MARK/a && echo hello > /tmp/$MARK/b && /bin/cat /tmp/$MARK/b > /dev/null && /bin/ls /tmp/$MARK > /dev/null; rm -rf /tmp/$MARK" >/dev/null 2>&1
  sleep 3
  guest_sudo "kill \$(cat /var/tmp/es-$l.pid) 2>/dev/null; sleep 1; head -c 2000000 /var/tmp/es-$l.json > /var/tmp/es-$l.head.json; chmod 644 /var/tmp/es-$l.head.json /var/tmp/es-$l.err" >/dev/null 2>&1
  guest_get "/var/tmp/es-$l.head.json" "$KIT_WORK/es-$l.json"
  guest_get "/var/tmp/es-$l.err" "$KIT_WORK/es-$l.err"
  kit_attach "$KIT_WORK/es-$l.err"; kit_attach "$KIT_WORK/es-$l.state"
  [ -s "$KIT_WORK/es-$l.json" ] && grep "$MARK" "$KIT_WORK/es-$l.json" | head -50 > "$KIT_WORK/es-$l-probe.json" && kit_attach "$KIT_WORK/es-$l-probe.json"
  TRY_STATE="$(cat "$KIT_WORK/es-$l.state" | tail -1)"
  TRY_LINES="$(wc -l < "$KIT_WORK/es-$l.json" | tr -d ' ')"
  TRY_PROBE="$(es_count "$KIT_WORK/es-$l.json")"
  TRY_EXEC="$(es_execs "$KIT_WORK/es-$l.json")"
  if es_refused "$KIT_WORK/es-$l.err"; then TRY_REFUSED=yes; else TRY_REFUSED=no; fi
  kit_fact "eslogger_$l" "state=$TRY_STATE refused=$TRY_REFUSED lines=$TRY_LINES probe_lines=$TRY_PROBE touch_exec_lines=$TRY_EXEC error=$(head -1 "$KIT_WORK/es-$l.err" | cut -c1-200)"
}

try plain
if [ "$TRY_PROBE" -gt 0 ] 2>/dev/null; then
  kit_outcome es-over-ssh-works "With no grant, eslogger ran as root over SSH and saw $TRY_PROBE lines about the probe ($TRY_EXEC exec of touch, $TRY_LINES lines in all)."
  exit 0
fi
plain_refused="$TRY_REFUSED"; plain_err="$(head -1 "$KIT_WORK/es-plain.err" | cut -c1-200)"

kit_human "In the computer's window on your Mac (the macOS desktop of user $VM_USER):
  1. Open System Settings > General > Sharing.
  2. Click the (i) next to Remote Login.
  3. Turn on \"Allow full disk access for remote users\". Confirm with the password if asked:
     the password is in $KIT_SSH_DIR/password on your Mac.
  4. Close System Settings."
kit_fact remote_fda_after "$(guest_sh "ls ~/Documents >/dev/null 2>&1 && echo documents-readable || echo documents-refused")"
try remote-fda

if [ "$TRY_PROBE" -gt 0 ] 2>/dev/null; then
  kit_outcome es-needs-remote-fda "Without the switch: refused=$plain_refused ($plain_err). With \"Allow full disk access for remote users\": $TRY_PROBE probe lines, $TRY_EXEC exec of touch."
elif [ "$TRY_REFUSED" = yes ]; then
  kit_outcome es-refused "Refused without and with the switch: $(head -1 "$KIT_WORK/es-remote-fda.err" | cut -c1-200)"
else
  kit_outcome inconclusive "Without the switch: refused=$plain_refused. With it: state=$TRY_STATE, $TRY_LINES lines, none about the probe; read the attached logs."
fi
