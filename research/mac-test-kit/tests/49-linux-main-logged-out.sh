#!/bin/bash
# id: 49
# title: Option B's main run by a launch daemon while you are logged out (#381 L7)
# tier: guided
# time: about 15 minutes; you log out for 3 minutes and log back in
# needs: Homebrew's libkrun and ./kit build, the image test 47 prepares, --allow-sudo (a launch daemon is installed and removed), you at the Mac
# step: later
# decides: engine-matches-wanted (computers that keep running while you are logged out), linux-vm-engine (whether libkrun's hypervisor use is allowed outside your login session)
# question: When krun-boot runs main as a launch daemon in the system domain (as your user, with the hypervisor entitlement), does main start, and does it keep running, with its clock and its writes going on, while you log out and back in?
# outcome runs-logged-out: Main started from launchd and wrote every 10 s while you were logged out: under B the engine can keep computers
#   going with nobody logged in.
# outcome stops-at-logout: Main started, but its writes stopped while you were logged out (or it was gone when you came back).
# outcome refused: launchd could not start main (the hypervisor refused outside a login session, or the job failed); the facts say how.
# outcome inconclusive: The logout was skipped, or main did not answer before it.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/krun.sh"
krun_need
[ "${KIT_SUDO:-0}" = 1 ] || kit_skip "needs --allow-sudo: the test installs a launch daemon (and removes it)"
V=k49; D="$KRUN_HOME/vms/$V"; L=com.silokit.krun49; P="/Library/LaunchDaemons/$L.plist"
cleanup() { sudo launchctl bootout "system/$L" 2>/dev/null; sudo rm -f "$P"; krun_destroy "$V"; }
trap cleanup EXIT
trap 'exit 130' INT TERM HUP
krun_new "$V" || kit_fail "could not make the computer"
init=""; [ -f "$KRUN_HOME/image/initrd" ] && init="<string>--initrd</string><string>$KRUN_HOME/image/initrd</string>"
cat > "$KIT_WORK/$L.plist" <<EOP
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>$L</string><key>UserName</key><string>$(id -un)</string>
<key>ProgramArguments</key><array><string>$KRUN_BOOT</string><string>--kernel</string><string>$KRUN_HOME/image/Image</string>$init
<string>--disk</string><string>$D/disk.raw</string><string>--seed</string><string>$D/seed.iso</string>
<string>--ssh-sock</string><string>$D/ssh.sock</string><string>--console</string><string>$D/console.log</string></array>
<key>RunAtLoad</key><true/><key>StandardErrorPath</key><string>$D/krun.log</string></dict></plist>
EOP
kit_sudo "install main as a launch daemon in the system domain, run as $(id -un)" cp "$KIT_WORK/$L.plist" "$P"
sudo chmod 644 "$P"; sudo chown root:wheel "$P"
kit_sudo "start it" launchctl bootstrap system "$P"
sleep 5
sudo launchctl print "system/$L" 2>/dev/null | grep -E '^\s*(state|pid|last exit code)' | tr -s ' ' > "$KIT_WORK/job.txt"
kit_fact launchd_job "$(tr '\n' ';' < "$KIT_WORK/job.txt")"
pid="$(awk '/pid =/{print $3}' "$KIT_WORK/job.txt")"; [ -n "$pid" ] && echo "$pid" > "$D/krun.pid"
krun_wait_ssh "$V" 300 >/dev/null || { kit_attach "$D/krun.log" 2>/dev/null; kit_outcome refused "main started from launchd never answered SSH ($(tail -1 "$D/krun.log" 2>/dev/null))"; exit 0; }
krun_ssh "$V" 'nohup sh -c "while :; do date +%s >> /home/kit/ticks; sync; sleep 10; done" >/dev/null 2>&1 &'
t0="$(date +%s)"
kit_human "Log out of macOS now (Apple menu, Log Out), stay logged out for 3 minutes, then log back in, open Terminal again and press Enter here."
t1="$(date +%s)"
[ $((t1 - t0)) -ge 120 ] || { kit_outcome inconclusive "the logout took under 2 minutes; nothing to read"; exit 0; }
ticks="$(krun_ssh "$V" 'cat /home/kit/ticks' 2>/dev/null)" || { kit_outcome stops-at-logout "main did not answer SSH after the logout"; exit 0; }
gap="$(echo "$ticks" | awk -v a="$t0" -v b="$t1" '$1 >= a && $1 <= b {if (p) {g = $1 - p; if (g > m) m = g}; p = $1} END {print m + 0}')"
kit_measure longest_gap_while_logged_out "$gap" s "between main's writes every 10 s"
if [ "$gap" -le 30 ]; then kit_outcome runs-logged-out "main kept writing every 10 s across $((t1 - t0)) s of logout (longest gap $gap s)"
else kit_outcome stops-at-logout "main's writes stopped for $gap s during the logout"; fi
