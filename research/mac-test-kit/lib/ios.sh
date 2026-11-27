# shellcheck shell=bash disable=SC2034  # its variables are read by the tests that source it
# Helpers for the tests of an iOS project in a computer (23 to 30). Sourced after lib/common.sh,
# lib/vm.sh and lib/guest.sh. Written for macOS /bin/bash 3.2.
#
# Test 23 makes "the Xcode computer": the installed computer of test 00 with Xcode copied in from
# this Mac and the iOS simulator runtime downloaded, stopped and kept at XCODE_VM. Tests 24 to 30
# start a clone of it and delete the clone at the end.
#
# On Linux with KIT_FAKE=1 (./kit selftest), XCODE_VM is a fake bundle and linux/fake-ios comes
# first on PATH: a fake xcrun (simctl, swiftc), vm_stat, sysctl, sysadminctl, security and stat,
# so the tests' logic runs against canned behaviour. Without KIT_FAKE=1 the tests skip on Linux.

XCODE_VM="${KIT_WORK_ROOT:-$HOME/SiloKit}/base/xcode.silovm"

# ios_fake: true when the tests run against the fakes of linux/fake-ios.
ios_fake() { guest_local && [ "${KIT_FAKE:-0}" = 1 ]; }

if ios_fake; then
  XCODE_VM="${KIT_WORK:-/tmp}/fake-xcode.silovm"
  [ -d "$XCODE_VM" ] || cp -R "$KIT_BASE_VM" "$XCODE_VM"
  PATH="$KIT_ROOT/linux/fake-ios:$PATH"
  export PATH
fi

# ios_need_mac_or_fake "why": skip on Linux unless the fakes are in use.
ios_need_mac_or_fake() {
  if guest_local; then
    ios_fake || kit_skip "needs a Mac: $1"
    kit_note "Linux run: xcrun, simctl and the macOS commands are the fakes of linux/fake-ios; this checks the test's logic only"
    return 0
  fi
  kit_need_mac 27
  kit_need_silovm
}

ios_need_xcode_vm() {
  [ -d "$XCODE_VM" ] || kit_skip "no Xcode computer yet: run ./kit run 23 first (copies Xcode in, about an hour)"
}

# ios_wait SECONDS: let the computer settle. No wait against the fakes.
ios_wait() { ios_fake || sleep "$1"; }

# ios_up NAME [silovm run options]: start a clone of the Xcode computer. Sets GB.
ios_up() {
  local name="$1"; shift
  ios_need_xcode_vm
  guest_forget_hosts
  guest_clone "$XCODE_VM" "$KIT_WORK/$name.silovm" || kit_fail "could not clone the Xcode computer"
  guest_start "$KIT_WORK/$name.silovm" "$@" || _guest_fail_with_logs "$KIT_WORK/$name.silovm" "the Xcode computer did not answer SSH within 10 minutes"
}

# The app the tests install: one SwiftUI screen, orange with a large text, so a screenshot that
# shows it is not blank. With SILOKIT_URL in its environment (simctl passes SIMCTL_CHILD_*), it
# fetches that URL once, writes "SILOKIT-FETCH ok <status>" or "SILOKIT-FETCH error <code>" to
# stderr and exits.
IOS_APP_SWIFT='import SwiftUI
import Foundation

@main
struct KitApp: App {
    init() {
        if let s = ProcessInfo.processInfo.environment["SILOKIT_URL"], let url = URL(string: s) {
            URLSession.shared.dataTask(with: url) { _, response, error in
                let line: String
                if let error = error as NSError? {
                    line = "SILOKIT-FETCH error \(error.code)\n"
                } else {
                    line = "SILOKIT-FETCH ok \((response as? HTTPURLResponse)?.statusCode ?? 0)\n"
                }
                FileHandle.standardError.write(line.data(using: .utf8)!)
                exit(0)
            }.resume()
        }
    }
    var body: some Scene {
        WindowGroup {
            ZStack {
                Color.orange.ignoresSafeArea()
                Text("Silo kit").font(.system(size: 64, weight: .bold)).foregroundColor(.black)
            }
        }
    }
}'

# _ios_plist PLATFORM: the app's Info.plist, for iPhoneSimulator or iPhoneOS.
_ios_plist() {
  cat <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.silo.kitapp</string>
<key>CFBundleExecutable</key><string>KitApp</string>
<key>CFBundleName</key><string>KitApp</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleSupportedPlatforms</key><array><string>$1</string></array>
<key>MinimumOSVersion</key><string>18.0</string>
<key>UIDeviceFamily</key><array><integer>1</integer></array>
<key>UILaunchScreen</key><dict/>
</dict></plist>
EOF
}

# ios_build_app [device]: write and build the app in the computer, at ~/silokit/KitApp.app (or
# ~/silokit/device/KitApp.app with "device"), with no Xcode project: swiftc for the simulator or
# for an iPhone, then an ad hoc signature. Prints the build's last lines, then "built".
ios_build_app() {
  local dir=silokit sdk=iphonesimulator target=arm64-apple-ios18.0-simulator platform=iPhoneSimulator
  if [ "${1:-}" = device ]; then
    dir=silokit/device sdk=iphoneos target=arm64-apple-ios18.0 platform=iPhoneOS
  fi
  local q_swift q_plist
  q_swift="$(printf '%s' "$IOS_APP_SWIFT" | base64 | tr -d '\n')"
  q_plist="$(_ios_plist "$platform" | base64 | tr -d '\n')"
  guest_sh "set -e; mkdir -p \$HOME/$dir/KitApp.app; cd \$HOME/$dir
echo '$q_swift' | base64 --decode > main.swift
echo '$q_plist' | base64 --decode > KitApp.app/Info.plist
xcrun --sdk $sdk swiftc -parse-as-library -target $target -o KitApp.app/KitApp main.swift 2>&1 | tail -5
codesign -f -s - KitApp.app 2>&1 | tail -2
test -x KitApp.app/KitApp && echo built"
}

# ios_runtime_and_type: print "<runtime id> <device type id>" for the newest iOS runtime installed
# and the newest iPhone it supports, read from simctl's JSON in the computer.
ios_runtime_and_type() {
  guest_sh 'xcrun simctl list -j runtimes devicetypes | /usr/bin/python3 -c "
import json, sys
d = json.load(sys.stdin)
rts = [r for r in d[\"runtimes\"] if r.get(\"platform\") == \"iOS\" and r.get(\"isAvailable\")]
if not rts: sys.exit(1)
rt = sorted(rts, key=lambda r: [int(x) for x in r[\"version\"].split(\".\")])[-1]
ok = set(t[\"identifier\"] for t in rt.get(\"supportedDeviceTypes\", []))
ts = [t for t in d[\"devicetypes\"] if t[\"identifier\"] in ok and t.get(\"productFamily\") == \"iPhone\"]
if not ts: sys.exit(1)
print(rt[\"identifier\"], ts[-1][\"identifier\"])
"'
}

# ios_new_device NAME: create a simulator device on the newest iOS runtime. Prints its UDID.
ios_new_device() {
  local name="$1" rt_type
  rt_type="$(ios_runtime_and_type)" || return 1
  # shellcheck disable=SC2086
  set -- $rt_type
  guest_sh "xcrun simctl create '$name' '$2' '$1'" 2>/dev/null | tail -1
}

# ios_console_user: who is logged in on the computer's screen ("none" when nobody).
ios_console_user() {
  guest_sh 'u=$(stat -f %Su /dev/console 2>/dev/null); case "$u" in ""|root) echo none ;; *) echo "$u" ;; esac'
}

# ios_screen_login [silovm run options]: log the provisioned user in on the computer's screen at
# every start (automatic login), stop the computer and start it again with the same options as
# its first start (--gui keeps its window), and print who is on the screen afterwards. Returns 1
# when the computer does not come back.
ios_screen_login() {
  guest_sudo "sysadminctl -autologin set -userName $VM_USER -password '$(vm_password)'; sync" >/dev/null 2>&1
  vm_stop "$GB" 60 >/dev/null
  guest_start "$GB" "$@" || return 1
  ios_wait 20
  ios_console_user
}

# ios_memory_used_mb: memory the computer's system counts as used (active + wired + compressed),
# in MB, then the swap used in MB, on one line.
ios_memory_used_mb() {
  guest_sh 'ps=$(sysctl -n hw.pagesize)
vm_stat | awk -v ps="$ps" "/Pages active/ {a=\$3} /Pages wired down/ {w=\$4} /occupied by compressor/ {c=\$5} END {gsub(/\\./,\"\",a); gsub(/\\./,\"\",w); gsub(/\\./,\"\",c); printf \"%d \", (a+w+c)*ps/1048576}"
sysctl -n vm.swapusage | sed -n "s/.*used = \\([0-9.]*\\)M.*/\\1/p"'
}

# ios_bmp_colours FILE: how many distinct colours a sample of a BMP screenshot holds (simctl io
# screenshot --type=bmp). 1 or 2 is a blank screen; 0 means the file is missing or not a BMP.
ios_bmp_colours() {
  python3 - "$1" <<'EOF'
import struct, sys
try:
    data = open(sys.argv[1], "rb").read()
except OSError:
    print(0); sys.exit(0)
if data[:2] != b"BM" or len(data) < 54:
    print(0); sys.exit(0)
off = struct.unpack("<I", data[10:14])[0]
bpp = struct.unpack("<H", data[28:30])[0]
if bpp not in (24, 32):
    print(0); sys.exit(0)
step = bpp // 8
px = data[off:]
n = len(px) // step
stride = max(1, n // 20000)
print(len(set(px[i * step:i * step + 3] for i in range(0, n, stride))))
EOF
}

# ios_trust_ca CA LEAF HOST LOG: trust a test authority in the computer's System keychain (never
# on the Mac), the three ways test 21 found it may take: as root over SSH; with the right
# com.apple.trust-settings.admin allowed for one call and put back; and, with a person at the
# keyboard, by them in the computer's window. CA and LEAF are paths in the computer. Prints how it
# got in ("root-over-ssh", "authorizationdb-allow", "a person") or "none".
ios_trust_ca() {
  local ca="$1" leaf="$2" host="$3" log="$4" way=none
  local add="security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain '$ca'"
  local trusted="security verify-cert -c '$leaf' -p ssl -s $host >/dev/null 2>&1"
  echo "# 1. as root over SSH" > "$log"
  guest_sudo "$GUEST_LIMIT limit 60 $add" >> "$log" 2>&1
  if guest_sh "$trusted"; then way=root-over-ssh; else
    echo "# 2. with com.apple.trust-settings.admin allowed for one call" >> "$log"
    guest_sudo "$GUEST_LIMIT security authorizationdb read com.apple.trust-settings.admin > /var/tmp/silokit-trust-right.plist 2>/dev/null; security authorizationdb write com.apple.trust-settings.admin allow; limit 60 $add; security authorizationdb write com.apple.trust-settings.admin < /var/tmp/silokit-trust-right.plist" >> "$log" 2>&1
    if guest_sh "$trusted"; then way=authorizationdb-allow; elif [ -t 0 ]; then
      kit_human "macOS refused to trust the test authority without a person. In the computer's window on your Mac:
  1. Open Terminal (Applications > Utilities).
  2. Run:  sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain $ca
     The sudo password, and the password of the dialog that follows, is in $KIT_SSH_DIR/password on your Mac.
  3. Approve the dialog (it changes trust settings inside that computer only)."
      if guest_sh "$trusted"; then way="a person"; fi
    fi
  fi
  echo "$way"
}

# ios_mcp_tools FILE: read the output of an MCP server on stdio (one JSON-RPC message per line) and
# print how many tools its answer to the request with id 2 (tools/list) lists, or "none" when no
# such answer came, or "error <message>" when it answered with an error.
ios_mcp_tools() {
  python3 - "$1" <<'EOF'
import json, sys
try:
    lines = open(sys.argv[1], errors="replace").read().splitlines()
except OSError:
    lines = []
for line in lines:
    line = line.strip()
    if not line.startswith("{"):
        continue
    try:
        m = json.loads(line)
    except ValueError:
        continue
    if m.get("id") != 2:
        continue
    if "error" in m:
        print("error " + str(m["error"].get("message", ""))[:120]); sys.exit(0)
    print(len((m.get("result") or {}).get("tools") or [])); sys.exit(0)
print("none")
EOF
}

# ios_devicectl_iphones FILE: read `devicectl list devices --json-output FILE` and print one line
# per iOS device: "<identifier> <pairing state> <transport> <tunnel state>".
ios_devicectl_iphones() {
  python3 - "$1" <<'EOF'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except (OSError, ValueError):
    sys.exit(0)
for dev in (d.get("result") or {}).get("devices") or []:
    hw = dev.get("hardwareProperties") or {}
    if hw.get("platform") != "iOS":
        continue
    c = dev.get("connectionProperties") or {}
    print(dev.get("identifier", "?"), c.get("pairingState", "?"), c.get("transportType", "?"), c.get("tunnelState", "?"))
EOF
}

# ios_usb_iphones FILE: read `system_profiler -json SPUSBHostDataType` (or SPUSBDataType) and print
# one line per USB device named like an iPhone: "<location id> <name>". The location is the first
# key whose name holds "location".
ios_usb_iphones() {
  python3 - "$1" <<'EOF'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except (OSError, ValueError):
    sys.exit(0)
def walk(x):
    if isinstance(x, dict):
        name = str(x.get("_name", ""))
        if "iphone" in name.lower():
            loc = next((str(v) for k, v in x.items() if "location" in k.lower()), "?")
            print(loc.split()[0], name.replace(" ", "_"))
        for v in x.values():
            walk(v)
    elif isinstance(x, list):
        for v in x:
            walk(v)
walk(d)
EOF
}
