#!/bin/bash
# id: 30
# title: An iPhone passed through USB to a computer
# tier: guided
# time: about 20 minutes, with you at the keyboard to plug in and unlock the iPhone and tap Trust
# needs: the Xcode computer of test 23, macOS 27 on the Mac, an iPhone and its cable, and a VM helper with --usb-device (VZUSBPassthroughDevice), which ./kit build does not make yet; while the computer runs, the iPhone is the computer's and not the Mac's
# decides: builds-on-your-iphone (whether an agent's build reaches your iPhone from inside the computer, or leaves it for the Mac)
# question: On macOS 27, does an iPhone given to a computer with USB passthrough show in the computer, pair with Xcode's device tools there, and take an install from it?
# outcome attaches-and-installs: The computer saw the iPhone, devicectl paired with it, and an
#   install reached the phone (it installed, or the phone refused only the build's signature, which
#   a development identity fixes): an agent's build can go to your iPhone from inside the computer,
#   with the phone handed to that computer while it runs.
# outcome attaches-not-paired: The computer saw the iPhone on USB, but devicectl did not list it or
#   could not pair: passthrough is not enough for Xcode, and builds-on-your-iphone takes the Mac
#   route (the app comes out to the Mac and is installed from there).
# outcome not-attached: The computer never saw the iPhone, as reported on the macOS 27 beta:
#   builds-on-your-iphone takes the Mac route.
# outcome attach-refused: The framework refused to attach the iPhone (the error is in the
#   sentence): builds-on-your-iphone takes the Mac route.
# outcome inconclusive: No iPhone was found on this Mac, or the computer did not start; read the facts.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"
. "$KIT_ROOT/lib/ios.sh"

W="$KIT_WORK"

if guest_local; then
  ios_fake || kit_skip "needs a Mac and an iPhone"
  kit_note "Linux run: the readers of system_profiler and devicectl, on canned output"
  cat > "$W/usb.json" <<'EOF'
{"SPUSBHostDataType":[{"_name":"USB31Bus","_items":[{"_name":"iPhone","USBDeviceKeyLocationID":"0x01100000","USBDeviceKeyVendorName":"Apple Inc."},{"_name":"Keyboard","USBDeviceKeyLocationID":"0x02100000"}]}]}
EOF
  cat > "$W/devices.json" <<'EOF'
{"result":{"devices":[{"identifier":"A1B2","hardwareProperties":{"platform":"iOS","productType":"iPhone17,1"},"connectionProperties":{"pairingState":"paired","transportType":"wired","tunnelState":"connected"}},{"identifier":"W1","hardwareProperties":{"platform":"watchOS"},"connectionProperties":{}}]}}
EOF
  u="$(ios_usb_iphones "$W/usb.json")"; d="$(ios_devicectl_iphones "$W/devices.json")"
  kit_measure linux_parsers_ok "$( [ "$u" = "0x01100000 iPhone" ] && [ "$d" = "A1B2 paired wired connected" ] && echo 1 || echo 0)" bool "an iPhone on USB, an iPhone in devicectl"
  kit_outcome inconclusive "Linux run: only the readers of system_profiler and devicectl ran."
  exit 0
fi

kit_need_mac 27
kit_need_silovm
ios_need_xcode_vm

# usb_on_mac FILE: this Mac's USB devices, as system_profiler's JSON.
usb_on_mac() { system_profiler -json SPUSBHostDataType > "$1" 2>/dev/null || system_profiler -json SPUSBDataType > "$1" 2>/dev/null; }
usb_on_mac "$W/mac-usb.json"
phones="$(ios_usb_iphones "$W/mac-usb.json")"
if [ -z "$phones" ]; then
  kit_human "Connect your iPhone to this Mac with a cable and unlock it."
  usb_on_mac "$W/mac-usb.json"
  phones="$(ios_usb_iphones "$W/mac-usb.json")"
fi
[ -n "$phones" ] || { kit_outcome inconclusive "no iPhone found on this Mac's USB"; exit 0; }
kit_fact iphone_on_mac "$(echo "$phones" | head -1)"
loc="$(echo "$phones" | head -1 | awk '{print $1}')"

"$SILOVM" help 2>&1 | grep -q -- '--usb-device' ||
  kit_skip "the VM helper cannot hand a USB device to a computer yet: silovm run needs --usb-device LOCATION (VZUSBPassthroughDevice); the iPhone was found on this Mac at $loc"

kit_confirm "This starts a clone of the Xcode computer with its screen in a window for about 20 minutes and hands it your iPhone ($loc) over USB: until it stops, the iPhone is the computer's, not this Mac's. Nothing is installed on the iPhone that stays: a test app at most, which you can delete."
trap guest_cleanup EXIT
GB="$KIT_WORK/iphone.silovm"
guest_forget_hosts
guest_clone "$XCODE_VM" "$GB" || kit_fail "could not clone the Xcode computer"
vm_start "$GB" --gui --usb-device "$loc" >/dev/null
if ! guest_wait_started "$GB" 120; then
  kit_attach "$GB/run.log"; kit_attach "$GB/run.jsonl"
  kit_outcome attach-refused "the computer did not start with the iPhone attached: $(grep -m1 -iE 'usb|error' "$GB/run.log" "$GB/run.jsonl" | cut -c1-200)"
  exit 0
fi
ssh_s="$(vm_wait_ssh "$GB" "$VM_USER" 600)" || { kit_attach "$GB/run.log"; kit_outcome inconclusive "the computer with the iPhone attached did not answer SSH"; exit 0; }
kit_measure seconds_to_ssh "$ssh_s" s "start with the iPhone attached"
grep -iE 'usb' "$GB/run.jsonl" > "$W/usb-events.txt" 2>/dev/null && kit_attach "$W/usb-events.txt"

# 1. Does the computer see it on USB?
guest_sh 'system_profiler -json SPUSBHostDataType 2>/dev/null || system_profiler -json SPUSBDataType' > "$W/guest-usb.json" 2>/dev/null
seen="$(ios_usb_iphones "$W/guest-usb.json")"
kit_fact iphone_in_computer_usb "${seen:-none}"

# 2. Does devicectl list it, and pair? The phone asks "Trust This Computer?".
list() { guest_sh "$GUEST_LIMIT limit 60 xcrun devicectl list devices --json-output \$HOME/silokit/devices.json >/dev/null 2>&1; cat \$HOME/silokit/devices.json" > "$W/devices.json" 2>/dev/null; ios_devicectl_iphones "$W/devices.json"; }
listed="$(list)"
kit_fact devicectl_before_trust "${listed:-none}"
if [ -n "$seen" ] || [ -n "$listed" ]; then
  kit_human "Unlock the iPhone. If it asks whether to trust this computer, tap Trust and enter its passcode."
  ios_wait 10
  listed="$(list)"
  kit_fact devicectl_after_trust "${listed:-none}"
fi
dev="$(echo "$listed" | head -1 | awk '{print $1}')"
paired="$(echo "$listed" | head -1 | awk '{print $2}')"

# 3. An install from the computer. The build is signed ad hoc, so the phone may refuse its
# signature; that refusal still shows the install reached it.
inst="not-run"
if [ -n "$dev" ]; then
  build="$(ios_build_app device 2>&1)"
  if echo "$build" | grep -q built; then
    inst="$(guest_sh "$GUEST_LIMIT limit 300 xcrun devicectl device install app --device '$dev' \$HOME/silokit/device/KitApp.app 2>&1; echo rc=\$?" | tr '\n' ' ' | guest_redact | cut -c1-400)"
  else
    inst="build failed: $(echo "$build" | tail -1 | cut -c1-160)"
  fi
fi
kit_fact install "$inst"

reached=no
case "$inst" in
  *rc=0*) reached=installed ;;
  *[Ss]ignature*|*[Pp]rovisioning*|*0xe800801*|*[Ee]ntitlement*) reached=refused-signature ;;
esac
summary="USB in the computer: ${seen:-none}; devicectl: ${listed:-none}; install: $reached"
if [ "$reached" != no ]; then kit_outcome attaches-and-installs "$summary"
elif [ -n "$listed" ] || [ -n "$seen" ]; then kit_outcome attaches-not-paired "$summary (pairing: ${paired:-none}); install said: $inst"
else kit_outcome not-attached "$summary"
fi
