#!/bin/bash
# Compiles the kit's programs into ~/SiloKit/bin. Run by ./kit build. Needs the Command Line Tools.
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin"
mkdir -p "$BIN"

if [ "$(uname -s)" != Darwin ]; then
  echo "On Linux only the C programs are built, with their Linux fallbacks (for ./kit selftest)." >&2
fi
if [ "$(uname -s)" = Darwin ] && ! xcode-select -p >/dev/null 2>&1; then
  echo "The Command Line Tools are missing. Run: xcode-select --install, then ./kit build again." >&2
  exit 1
fi

fail=0
for src in "$KIT_ROOT"/c/*.c; do
  [ -f "$src" ] || continue
  name="$(basename "$src" .c)"
  if cc -O2 -Wall -o "$BIN/$name" "$src" 2> "$BIN/$name.build.log"; then
    echo "built $name" >&2
  else
    echo "FAILED $name: see $BIN/$name.build.log" >&2; cat "$BIN/$name.build.log" >&2; fail=1
  fi
done

if [ "$(uname -s)" = Darwin ]; then
  if swiftc -O -parse-as-library -framework Virtualization -framework AppKit \
       -o "$BIN/silovm" "$KIT_ROOT"/swift/*.swift 2> "$BIN/silovm.build.log" \
     && codesign --force --sign - --entitlements "$KIT_ROOT/swift/silovm.entitlements" "$BIN/silovm" 2>> "$BIN/silovm.build.log"; then
    echo "built silovm" >&2
    hc="$("$BIN/silovm" host-check || true)"; echo "$hc"
    major="$(sw_vers -productVersion | cut -d. -f1)"
    if [ "$major" -ge 27 ] && printf '%s' "$hc" | grep -q '"provisioning_api" *: *false'; then
      echo "WARNING: this swiftc ($(swiftc --version 2>&1 | head -1)) is older than Swift 6.4, so silovm was" >&2
      echo "built without guest provisioning and the VM tests will fail. Update the Command Line Tools," >&2
      echo "or select Xcode 27 with sudo xcode-select -s /Applications/Xcode.app, then ./kit build again." >&2
      fail=1
    fi
  else
    echo "FAILED silovm: see $BIN/silovm.build.log" >&2; cat "$BIN/silovm.build.log" >&2; fail=1
  fi
  # krun-boot, for tests 47 to 49 (option B, #381), only when Homebrew's libkrun is installed.
  kp="$(brew --prefix libkrun 2>/dev/null || true)"
  if [ -n "$kp" ] && [ -f "$kp/include/libkrun.h" ]; then
    if cc -O2 -Wall -I "$kp/include" -L "$kp/lib" -o "$BIN/krun-boot" "$KIT_ROOT/krun/krun-boot.c" -lkrun 2> "$BIN/krun-boot.build.log" \
       && codesign --force --sign - --entitlements "$KIT_ROOT/krun/krun.entitlements" "$BIN/krun-boot" 2>> "$BIN/krun-boot.build.log"; then
      echo "built krun-boot" >&2
    else
      echo "FAILED krun-boot: see $BIN/krun-boot.build.log" >&2; cat "$BIN/krun-boot.build.log" >&2; fail=1
    fi
  else
    echo "krun-boot not built: Homebrew's libkrun is not installed (tests 47 to 49 will skip;" >&2
    echo "brew tap slp/krun && brew install libkrun, then ./kit build again)." >&2
  fi
fi
exit $fail
