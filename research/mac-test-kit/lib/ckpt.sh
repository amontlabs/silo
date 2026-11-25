# The checkpoint engine's prototype (research/2026-11-24-checkpoint-engine/proto, in Zig), built
# for tests 31 and 32. Sourced after lib/common.sh.
#
# Its source is not in the kit. ckpt_build looks for it next to the kit (once both branches are
# merged), else fetches it from this repository's remote into the test's work folder. Zig 0.15.2
# comes from PATH when its version matches, else from ziglang.org into ~/SiloKit/bin, checked
# against the checksum below. The built program is ~/SiloKit/bin/ckpt, which test 32 reuses.

CKPT_BRANCH="claude/project-thread-uhc74m"
CKPT_ZIG_VERSION="0.15.2"
# https://ziglang.org/download/index.json, "0.15.2" > "aarch64-macos", read on 24 November 2026.
CKPT_ZIG_URL="https://ziglang.org/download/0.15.2/zig-aarch64-macos-0.15.2.tar.xz"
CKPT_ZIG_SHA256="3cc2bab367e185cdfb27501c4b30b1b0653c28d9f73df8dc91488e66ece5fa6b"
CKPT_ZIG_MB=51
CKPT_BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin/ckpt"

# ckpt_sha256 FILE: its SHA-256 in hex, with the Mac's shasum or Linux's sha256sum.
ckpt_sha256() {
  if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'
  else sha256sum "$1" | awk '{print $1}'; fi
}

# These functions set variables rather than print, so that kit_confirm and kit_fail act on the
# test itself and never on a subshell.

# ckpt_zig: sets CKPT_ZIG to a Zig 0.15.2, downloading it (after asking) when none is found.
# KIT_ZIG names one to use.
CKPT_ZIG=""
ckpt_zig() {
  local z dir tarball got
  for z in "${KIT_ZIG:-}" "$(command -v zig 2>/dev/null)" \
           "${KIT_WORK_ROOT:-$HOME/SiloKit}/bin/zig-aarch64-macos-$CKPT_ZIG_VERSION/zig"; do
    [ -n "$z" ] && [ -x "$z" ] || continue
    if [ "$("$z" version 2>/dev/null)" = "$CKPT_ZIG_VERSION" ]; then CKPT_ZIG="$z"; return 0; fi
    kit_log "$z is Zig $("$z" version 2>/dev/null), the prototype needs $CKPT_ZIG_VERSION: not used"
  done
  kit_is_mac && [ "$(uname -m)" = arm64 ] || { kit_log "no Zig $CKPT_ZIG_VERSION, and the download is for Apple silicon only"; return 1; }
  kit_confirm "The checkpoint engine's prototype needs Zig $CKPT_ZIG_VERSION: download it from ziglang.org (about $CKPT_ZIG_MB MB) into ${KIT_WORK_ROOT:-$HOME/SiloKit}/bin."
  dir="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin"
  mkdir -p "$dir"
  tarball="$dir/zig-$CKPT_ZIG_VERSION.tar.xz"
  curl -fL --retry 2 -o "$tarball" "$CKPT_ZIG_URL" >&2 || { rm -f "$tarball"; kit_log "the Zig download failed"; return 1; }
  got="$(ckpt_sha256 "$tarball")"
  if [ "$got" != "$CKPT_ZIG_SHA256" ]; then
    rm -f "$tarball"
    kit_fail "the Zig download's SHA-256 is $got, not the pinned $CKPT_ZIG_SHA256: deleted, not used"
  fi
  tar -xJf "$tarball" -C "$dir" || { kit_log "could not unpack $tarball"; return 1; }
  rm -f "$tarball"
  z="$dir/zig-aarch64-macos-$CKPT_ZIG_VERSION/zig"
  [ "$("$z" version 2>/dev/null)" = "$CKPT_ZIG_VERSION" ] || { kit_log "the unpacked Zig does not run"; return 1; }
  CKPT_ZIG="$z"
}

# ckpt_source: sets CKPT_SRC to the folder of the prototype's source (with build.zig).
# KIT_CKPT_PROTO names one to use. Fetching asks first.
CKPT_SRC=""
ckpt_source() {
  local d="${KIT_CKPT_PROTO:-$KIT_ROOT/../2026-11-24-checkpoint-engine/proto}" rev
  if [ -f "$d/build.zig" ]; then CKPT_SRC="$(cd "$d" && pwd)"; kit_fact prototype_source "$CKPT_SRC"; return 0; fi
  command -v git >/dev/null 2>&1 && git -C "$KIT_ROOT" rev-parse --git-dir >/dev/null 2>&1 || {
    kit_log "the prototype is not next to the kit and the kit is not in a git checkout to fetch it from"; return 1; }
  kit_confirm "The checkpoint engine's prototype is not next to the kit: fetch the branch $CKPT_BRANCH from this repository's remote (a few MB) and unpack its proto/ into $KIT_WORK."
  git -C "$KIT_ROOT" fetch -q origin "$CKPT_BRANCH" >&2 || { kit_log "git fetch of $CKPT_BRANCH failed"; return 1; }
  rev="$(git -C "$KIT_ROOT" rev-parse FETCH_HEAD)" || return 1
  rm -rf "$KIT_WORK/src"; mkdir -p "$KIT_WORK/src"
  git -C "$(git -C "$KIT_ROOT" rev-parse --show-toplevel)" archive "$rev" research/2026-11-24-checkpoint-engine/proto | tar -x -C "$KIT_WORK/src" || {
    kit_log "git archive of the prototype failed"; return 1; }
  kit_fact prototype_commit "$rev"
  CKPT_SRC="$KIT_WORK/src/research/2026-11-24-checkpoint-engine/proto"
}

# ckpt_build: builds the prototype into $CKPT_BIN. Returns 1 (after saying why) when it cannot.
# Zig's caches go under the work folder, never ~/.cache.
ckpt_build() {
  local z src
  ckpt_zig || return 1
  ckpt_source || return 1
  z="$CKPT_ZIG" src="$CKPT_SRC"
  kit_fact zig "$z ($("$z" version))"
  mkdir -p "$KIT_WORK/zig-out" "$(dirname "$CKPT_BIN")"
  if ! (cd "$src" && ZIG_GLOBAL_CACHE_DIR="$KIT_WORK/zig-cache" ZIG_LOCAL_CACHE_DIR="$KIT_WORK/zig-cache-local" \
          "$z" build --release -p "$KIT_WORK/zig-out") > "$KIT_WORK/zig-build.log" 2>&1; then
    kit_attach "$KIT_WORK/zig-build.log"
    kit_log "zig build failed (see zig-build.log)"
    return 1
  fi
  cp "$KIT_WORK/zig-out/bin/ckpt" "$CKPT_BIN" || return 1
  rm -rf "$KIT_WORK/zig-cache" "$KIT_WORK/zig-cache-local" "$KIT_WORK/zig-out"
  return 0
}
