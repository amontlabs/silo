#!/bin/bash
# id: 23
# title: Xcode copied in from the Mac, and the iOS runtime downloaded in the computer
# tier: vm
# time: about an hour, most of it copying Xcode and downloading the runtime
# needs: the installed computer of test 00 (80 GB disk), macOS 27 on the Mac, Xcode installed on this Mac (read, never changed), about 40 GB free, network (the runtime comes from Apple's CDN, about 9 GB)
# decides: xcode-in-a-computer, first-run-time-macos, computer-image (whether the kept image can hold Xcode)
# question: Does Xcode copied from this Mac into a computer pass Apple's signature check, accept its license and finish its first launch over SSH with nobody at the screen, and does the iOS simulator runtime download there with no Apple Account?
# outcome ready: Xcode passed the signature check, its license and first launch were done over SSH,
#   and the iOS runtime downloaded and shows as available. Bringing Xcode in from the Mac works with
#   nobody clicking; the sizes and times are in the measures. The computer is kept as the Xcode
#   computer for tests 24 to 30.
# outcome signature-refused: codesign or Gatekeeper refused the copied Xcode in the computer. A copy
#   over SSH loses something (an extended attribute, a signature seal), so Silo must import the .xip
#   instead and expand it inside.
# outcome first-launch-failed: Xcode passed the signature check but its license or first launch
#   failed or waited on the screen over SSH. xcode-in-a-computer needs a step on the computer's
#   screen, or another way to finish first launch; the sentence says which step.
# outcome runtime-refused: Xcode is ready but the runtime download failed in the computer (the
#   error is in the sentence). If it asks for an account, xcode-in-a-computer must bring the runtime
#   in from the Mac too.
# outcome inconclusive: Something else went wrong; the sentence and the attached logs say what.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"
. "$KIT_ROOT/lib/ios.sh"

guest_local && kit_skip "needs a Mac: it copies this Mac's Xcode into a macOS computer"
kit_need_mac 27
kit_need_silovm; kit_need_base_vm

XCODE="${KIT_XCODE:-$(xcode-select -p 2>/dev/null | sed 's#/Contents/Developer$##')}"
case "$XCODE" in
  *.app) ;;
  *) XCODE=/Applications/Xcode.app ;;
esac
[ -d "$XCODE" ] || kit_skip "no Xcode at $XCODE: install Xcode on this Mac, or set KIT_XCODE=/path/to/Xcode.app"
kit_fact mac_xcode "$("$XCODE/Contents/Developer/usr/bin/xcodebuild" -version 2>/dev/null | tr '\n' ' ')"
host_kb="$(du -sk "$XCODE" | awk '{print $1}')"
kit_measure xcode_on_mac_gb "$(python3 -c "print(round($host_kb / 1048576, 2))")" GB "$XCODE"
kit_need_free_gb 40

kit_confirm "This starts one macOS computer, copies $XCODE into it (about $((host_kb / 1048576)) GB; your Xcode is only read), downloads the iOS simulator runtime from Apple (about 9 GB) and keeps the result at $XCODE_VM."
trap guest_cleanup EXIT
guest_up xcode
kit_measure seconds_to_ssh "$GUEST_SSH_SECONDS" s "first start with provisioning"
disk_before="$(guest_sh 'df -k / | awk "NR==2 {print \$3}"')"

# 1. Copy. tar keeps extended attributes and the bundle as it is; the copy lands in the user's
# home and is moved to /Applications with sudo inside the computer.
log="$KIT_WORK/copy.log"
t0="$(kit_now)"
if ! ( cd "$(dirname "$XCODE")" && tar -cf - "$(basename "$XCODE")" ) |
  vm_ssh "$GB" "$VM_USER" "mkdir -p \$HOME/silokit-in && tar -C \$HOME/silokit-in -xf -" > "$log" 2>&1; then
  kit_attach "$log"
  kit_outcome inconclusive "the copy of Xcode into the computer failed: $(tail -1 "$log" | cut -c1-200)"
  exit 0
fi
t1="$(kit_now)"
kit_measure copy_seconds "$(python3 -c "print(round($t1 - $t0, 1))")" s "tar over SSH, $((host_kb / 1048576)) GB"
guest_sudo "rm -rf /Applications/Xcode.app && mv '/Users/$VM_USER/silokit-in/$(basename "$XCODE")' /Applications/Xcode.app && rmdir /Users/$VM_USER/silokit-in" ||
  kit_fail "could not move Xcode into /Applications in the computer"

# 2. Signature: the same checks Gatekeeper and codesign make.
sig="$KIT_WORK/signature.log"
guest_sh "$GUEST_LIMIT"' limit 1800 codesign --verify --deep --strict /Applications/Xcode.app; echo "codesign=$?"; spctl -a -vv /Applications/Xcode.app; echo "spctl=$?"' > "$sig" 2>&1
kit_attach "$sig"
if ! grep -q '^codesign=0' "$sig" || ! grep -q '^spctl=0' "$sig"; then
  kit_outcome signature-refused "$(grep -v '^$' "$sig" | tail -3 | tr '\n' ' ' | cut -c1-300)"
  exit 0
fi

# 3. License and first launch, as root, over SSH, each with a limit so a dialog on the screen shows
# as a time-out instead of a hang.
first="$KIT_WORK/first-launch.log"
guest_sudo "$GUEST_LIMIT"' xcode-select -s /Applications/Xcode.app; echo "select=$?"
limit 300 xcodebuild -license accept; echo "license=$?"
limit 1200 xcodebuild -runFirstLaunch; echo "firstlaunch=$?"
DevToolsSecurity -enable >/dev/null 2>&1; echo "devtools=$?"' > "$first" 2>&1
kit_attach "$first"
for step in select license firstlaunch; do
  rc="$(sed -n "s/^$step=//p" "$first")"
  if [ "$rc" != 0 ]; then
    kit_outcome first-launch-failed "step $step ended with ${rc:-nothing} (124 is the time limit, which suggests a dialog on the screen): $(grep -v '=' "$first" | tail -2 | tr '\n' ' ' | cut -c1-200)"
    exit 0
  fi
done
kit_fact devtools_security "status $(sed -n 's/^devtools=//p' "$first")"
kit_fact computer_xcode "$(guest_sh 'xcodebuild -version' 2>/dev/null | tr '\n' ' ')"

# 4. The runtime, from Apple's CDN, with no account. Apple silicon only when the flag exists.
rt="$KIT_WORK/runtime.log"
t0="$(kit_now)"
guest_sh "$GUEST_LIMIT"' limit 5400 xcodebuild -downloadPlatform iOS -architectureVariant arm64 || limit 5400 xcodebuild -downloadPlatform iOS; echo "download=$?"' > "$rt" 2>&1
t1="$(kit_now)"
kit_attach "$rt"
kit_measure runtime_download_seconds "$(python3 -c "print(round($t1 - $t0, 1))")" s "xcodebuild -downloadPlatform iOS"
if ! grep -q '^download=0' "$rt" || ! ios_runtime_and_type > /dev/null; then
  kit_outcome runtime-refused "$(grep -v '^$' "$rt" | tail -2 | tr '\n' ' ' | cut -c1-300)"
  exit 0
fi
kit_fact runtimes "$(guest_sh 'xcrun simctl runtime list' 2>/dev/null | grep -i ios | head -3 | tr '\n' ' ')"

disk_after="$(guest_sh 'df -k / | awk "NR==2 {print \$3}"')"
in_kb="$(guest_sh 'du -sk /Applications/Xcode.app 2>/dev/null' | awk '{print $1}')"
kit_measure xcode_in_computer_gb "$(python3 -c "print(round(${in_kb:-0} / 1048576, 2))")" GB "installed, after first launch"
kit_measure disk_added_gb "$(python3 -c "print(round((${disk_after:-0} - ${disk_before:-0}) / 1048576, 2))")" GB "Xcode, first launch and the iOS runtime, as the computer's file system counts it"

# 5. Keep it: stop, clone to XCODE_VM (outside the cleanup), delete the run's copy.
vm_stop "$GB" >/dev/null 2>&1
rm -rf "$XCODE_VM"
"$SILOVM" clone --from "$GB" --to "$XCODE_VM" >&2 || kit_fail "could not keep the Xcode computer at $XCODE_VM"
rm -f "$XCODE_VM/run.pid" "$XCODE_VM/ctl.sock"
kit_measure xcode_computer_on_mac_gb "$(du -sk "$XCODE_VM" | awk '{printf "%.1f", $1/1048576}')" GB "space the kept Xcode computer takes on this Mac"
kit_outcome ready "Xcode from this Mac passed codesign and Gatekeeper in the computer, license and first launch ran over SSH, and the iOS runtime downloaded with no account. Kept at $XCODE_VM."
