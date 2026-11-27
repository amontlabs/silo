#!/bin/sh
# id: K00
# title: The host can run the kit
# needs: nothing
# question: Is there a usable /dev/kvm, libkrun, gvproxy, socat, the kit's kernel and image?
TEST=K00; . "$(dirname "$0")/../lib.sh"
[ -c /dev/kvm ] || fail "no /dev/kvm. A cloud VM needs nested virtualization, or use a metal host (#366, section 1)"
[ -w /dev/kvm ] || fail "/dev/kvm not writable: add this user to the kvm group"
for t in socat gvproxy ssh ssh-keygen bc python3; do command -v $t >/dev/null || fail "missing $t"; done
[ -x "$KIT/bin/krun-boot" ] && [ -f "$KIT/kernel/image" ] && [ -f "$KIT/image/disk.raw" ] || fail "run setup.sh first"
result K00 passed "$(uname -m), kernel $(uname -r), libkrun $(pkg-config --modversion libkrun 2>/dev/null || echo unknown)"
