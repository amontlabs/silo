#!/usr/bin/env bash
# Builds a static `silo` for aarch64 Linux guests (the in-guest client) on an Apple silicon Mac,
# using Zig as the C compiler, archiver and linker. Needs `rustup target add aarch64-unknown-linux-musl`
# and `zig` on PATH. Prints the binary's path.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
command -v zig >/dev/null || { echo "zig is not on PATH" >&2; exit 1; }
tools=$(mktemp -d "${TMPDIR:-/tmp}/silo-guest-cc.XXXXXX")
trap 'rm -rf "$tools"' EXIT
cat > "$tools/cc" <<'CC'
#!/usr/bin/env python3
import os, sys
skip = ("--target=", "-Wl,--fix-cortex-a53-843419")
args = [a for a in sys.argv[1:] if not a.startswith(skip)]
os.execvp("zig", ["zig", "cc", "-target", "aarch64-linux-musl"] + args)
CC
printf '#!/bin/sh\nexec zig ar "$@"\n' > "$tools/ar"
chmod +x "$tools/cc" "$tools/ar"
cd "$root"
CC_aarch64_unknown_linux_musl="$tools/cc" \
AR_aarch64_unknown_linux_musl="$tools/ar" \
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="$tools/cc" \
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-C link-self-contained=no" \
RUSTC_WRAPPER= \
  cargo build --release -p silo-cli --target aarch64-unknown-linux-musl --locked >&2
echo "$root/target/aarch64-unknown-linux-musl/release/silo"
