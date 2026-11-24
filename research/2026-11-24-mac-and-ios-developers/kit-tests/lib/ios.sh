# shellcheck shell=bash disable=SC2034  # its variables are read by the tests that source it
# Helpers for the tests of an iOS project in a computer (23 to 27). Sourced after lib/common.sh,
# lib/vm.sh and lib/guest.sh. Written for macOS /bin/bash 3.2.
#
# Test 23 makes "the Xcode computer": the installed computer of test 00 with Xcode copied in from
# this Mac and the iOS simulator runtime downloaded, stopped and kept at XCODE_VM. Tests 24 to 27
# start a clone of it and delete the clone at the end.

XCODE_VM="${KIT_WORK_ROOT:-$HOME/SiloKit}/base/xcode.silovm"

ios_need_xcode_vm() {
  [ -d "$XCODE_VM" ] || kit_skip "no Xcode computer yet: run ./kit run 23 first (copies Xcode in, about an hour)"
}

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

IOS_APP_PLIST='<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.silo.kitapp</string>
<key>CFBundleExecutable</key><string>KitApp</string>
<key>CFBundleName</key><string>KitApp</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleSupportedPlatforms</key><array><string>iPhoneSimulator</string></array>
<key>MinimumOSVersion</key><string>18.0</string>
<key>UIDeviceFamily</key><array><integer>1</integer></array>
<key>UILaunchScreen</key><dict/>
</dict></plist>'

# ios_build_app: write and build the app in the computer, at ~/silokit/KitApp.app, with no Xcode
# project: swiftc for the simulator, then an ad hoc signature. Prints the build's last lines.
ios_build_app() {
  local q_swift q_plist
  q_swift="$(printf '%s' "$IOS_APP_SWIFT" | base64 | tr -d '\n')"
  q_plist="$(printf '%s' "$IOS_APP_PLIST" | base64 | tr -d '\n')"
  guest_sh "set -e; mkdir -p \$HOME/silokit/KitApp.app; cd \$HOME/silokit
echo '$q_swift' | base64 --decode > main.swift
echo '$q_plist' | base64 --decode > KitApp.app/Info.plist
xcrun --sdk iphonesimulator swiftc -parse-as-library -target arm64-apple-ios18.0-simulator -o KitApp.app/KitApp main.swift 2>&1 | tail -5
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
  local rt_type
  rt_type="$(ios_runtime_and_type)" || return 1
  # shellcheck disable=SC2086
  set -- $rt_type
  guest_sh "xcrun simctl create '$1' '$2' '$1'" 2>/dev/null | tail -1
}

# ios_console_user: who is logged in on the computer's screen ("none" when nobody).
ios_console_user() {
  guest_sh 'u=$(stat -f %Su /dev/console 2>/dev/null); case "$u" in ""|root) echo none ;; *) echo "$u" ;; esac'
}

# ios_memory_used_mb: memory the computer's system counts as used (active + wired + compressed),
# in MB, then the swap used in MB, on one line.
ios_memory_used_mb() {
  guest_sh 'ps=$(sysctl -n hw.pagesize)
vm_stat | awk -v ps="$ps" "/Pages active/ {a=\$3} /Pages wired down/ {w=\$4} /occupied by compressor/ {c=\$5} END {gsub(/\\./,\"\",a); gsub(/\\./,\"\",w); gsub(/\\./,\"\",c); printf \"%d \", (a+w+c)*ps/1048576}"
sysctl -n vm.swapusage | sed -n "s/.*used = \\([0-9.]*\\)M.*/\\1/p"'
}
