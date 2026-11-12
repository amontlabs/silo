#!/usr/bin/env bash
# Tests runtime-cache: the prepared runtime and the guest silo (runtime/guest-silo) are stored under
# their own keys, restored together, and a stale or unavailable guest silo is never served.
# A temporary tree and stubbed rustc/cargo/go/node/zig only.
set -uo pipefail
A=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
W=$(mktemp -d "${TMPDIR:-/tmp}/runtime-cache-test.XXXXXX"); W=$(cd "$W" && pwd -P); trap 'rm -rf "$W"' EXIT
unset RUNTIME_PREPARE_CMD SILO_RUNTIME_TARGET TAURI_ENV_TARGET_TRIPLE SILO_ZIG RUSTFLAGS
export SILO_AGENT_CACHE=$W/cache
N=0; BAD=0
ok() { N=$((N+1)); if [ "$2" = 0 ]; then echo "PASS $1"; else echo "FAIL $1 ${3:-}"; BAD=$((BAD+1)); fi; }
t() { if eval "$2"; then ok "$1" 0; else ok "$1" 1 "${3:-}"; fi; }

R=$W/repo; S=$R/app/SiloUI; T=$S/src-tauri; mkdir -p $R/tools/agent $S/scripts $S/src-tauri/guest/macos $S/src-tauri/macos-gateway $S/cli/src $S/other
cp $A/runtime-cache $R/tools/agent/
echo '{"toolchain":"1.95.0","patches":[]}' >$S/runtime-inputs.json
for f in package.json scripts/prepare-microsandbox-runtime.mjs scripts/microsandbox-runtime.mjs scripts/git-runtime.mjs scripts/linux-package-tools.mjs \
  scripts/build-input.mjs scripts/preflight.mjs scripts/guest-silo-runtime.mjs scripts/stage-guest-silo.mjs src-tauri/guest/macos/build-network-bootstrap.sh \
  src-tauri/guest/macos/silo-network-bootstrap.swift src-tauri/macos-gateway/a Cargo.toml Cargo.lock; do echo "$f" >$S/$f; done
echo "const goSelection = 'go 1.25'" >$S/scripts/lfs-transfer-runtime.mjs
echo lib1 >$S/cli/src/lib.rs; echo unrelated1 >$S/other/lib.rs

mkdir -p $W/bin
cat >$W/bin/rustc <<'X'
#!/bin/sh
case "$*" in *host-tuple*) echo aarch64-apple-darwin;; *) echo "rustc 1.95.0";; esac
X
cat >$W/bin/cargo <<X
#!/bin/sh
echo '{"workspace_root":"$S","packages":[{"id":"cli","name":"silo-cli","source":null,"manifest_path":"$S/cli/Cargo.toml"},{"id":"other","name":"other","source":null,"manifest_path":"$S/other/Cargo.toml"}],"resolve":{"nodes":[{"id":"cli","deps":[]},{"id":"other","deps":[]}]}}'
X
cat >$W/bin/go <<'X'
#!/bin/sh
echo '{}'
X
cat >$W/bin/zig <<'X'
#!/bin/sh
echo 0.15.2
X
# node stands in for stage-guest-silo.mjs: counts builds, builds a program unless ZIG_GONE, fails like CI when required.
cat >$W/bin/node <<X
#!/bin/sh
echo run >>$W/builds
if [ -n "\$ZIG_GONE" ]; then
  [ -n "\$FAIL_HARD" ] && exit 1
  mkdir -p $T/runtime/guest-silo && echo '{"available":false}' >$T/runtime/guest-silo/manifest.json; exit 1
fi
mkdir -p $T/runtime/guest-silo && echo '{"available":true}' >$T/runtime/guest-silo/manifest.json && echo "silo \$(cat $S/cli/src/lib.rs)" >$T/runtime/guest-silo/silo
X
cat >$W/prepare.sh <<X
#!/bin/sh
mkdir -p $T/binaries $T/runtime/microsandbox $T/runtime/guest-silo
echo msb >$T/binaries/msb-aarch64-apple-darwin; echo m >$T/runtime/microsandbox/manifest.json; echo '{}' >$T/runtime/release-info.json
echo '{"available":true}' >$T/runtime/guest-silo/manifest.json; echo prepared >$T/runtime/guest-silo/silo
X
chmod +x $W/bin/* $W/prepare.sh
export PATH=$W/bin:$PATH
rc() { (cd $R && python3 -I tools/agent/runtime-cache "$@") 2>&1; }
wipe() { rm -rf $T/binaries $T/runtime; }

wipe; rc --status >$W/s; t "status on an empty store is a miss for both" 'grep -qx miss $W/s && grep -q "^runtime miss" $W/s && grep -q "^guest-silo miss" $W/s' "$(cat $W/s)"
RUNTIME_PREPARE_CMD=$W/prepare.sh rc >$W/o1; t "a miss runs prepare and saves the runtime and the guest silo" 'grep -q "saved guest-silo" $W/o1 && [ ! -e $W/builds ]' "$(cat $W/o1)"
wipe; rc >$W/o2
t "a hit restores the runtime and the guest silo" 'grep -q "^runtime-cache: hit [0-9a-f]*, restored" $W/o2 && grep -q "hit guest-silo" $W/o2 && [ "$(cat $T/runtime/guest-silo/silo)" = prepared ] && [ ! -e $W/builds ]' "$(cat $W/o2)"
rc --status >$W/s; t "status says hit only when both are cached" 'grep -qx hit $W/s'

echo lib2 >$S/cli/src/lib.rs; wipe; rc >$W/o3
t "a changed silo-cli source misses the guest silo and rebuilds it (never serves the old program)" 'grep -q "miss guest-silo" $W/o3 && [ "$(cat $T/runtime/guest-silo/silo)" = "silo lib2" ] && [ "$(wc -l <$W/builds | tr -d " ")" = 1 ]' "$(cat $W/o3)"
rc --status >$W/s; t "the rebuilt program is saved: next restore hits" 'grep -qx hit $W/s'
echo unrelated2 >$S/other/lib.rs; rc --status >$W/s; t "a crate outside silo-cli's closure does not change the guest key" 'grep -q "^guest-silo hit" $W/s' "$(cat $W/s)"

echo lib3 >$S/cli/src/lib.rs; wipe; ZIG_GONE=1 rc >$W/o4; code=$?
t "without Zig the unavailable manifest stays and nothing is saved" '[ $code = 0 ] && grep -q "not available" $W/o4 && grep -q false $T/runtime/guest-silo/manifest.json && [ ! -e $T/runtime/guest-silo/silo ] && ls $SILO_AGENT_CACHE/runtime | grep -c guest-silo | grep -qx 2' "$(cat $W/o4) $(ls $SILO_AGENT_CACHE/runtime)"
wipe; ZIG_GONE=1 FAIL_HARD=1 rc >$W/o5; code=$?
t "a build that fails outright exits nonzero and leaves no stale guest silo" '[ $code != 0 ] && [ ! -e $T/runtime/guest-silo ]' "code=$code $(cat $W/o5)"
echo "$N tests, $BAD failed"; [ $BAD = 0 ]
