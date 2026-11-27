# The record's writer (research/2026-11-25-record-format/proto, `silorec`, in Zig), built for tests
# 40 and 41. Sourced after lib/common.sh and lib/ckpt.sh, whose Zig it uses (ckpt_zig).
#
# Its source is looked for next to the kit, else fetched from this repository's remote. With
# SILOREC_FLUSH=full|barrier|fsync it flushes each way on a Mac; `silorec bench` times an
# acknowledgement, `silorec verify` checks a record.

SILOREC_BRANCH="claude/project-thread-aev0v1"
SILOREC_BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin/silorec"

# silorec_build [TARGET]: builds the writer into $SILOREC_BIN (or, with a Zig target such as
# aarch64-macos, into $SILOREC_BIN-TARGET). Returns 1 (after saying why) when it cannot.
silorec_build() {
  local z src="${KIT_SILOREC_PROTO:-$KIT_ROOT/../2026-11-25-record-format/proto}" rev out="$SILOREC_BIN" tflag=""
  [ -n "${1:-}" ] && { out="$SILOREC_BIN-$1"; tflag="-Dtarget=$1"; }
  ckpt_zig || return 1
  z="$CKPT_ZIG"
  if [ ! -f "$src/build.zig" ] || ! grep -q '"bench"' "$src/src/main.zig" 2>/dev/null; then
    command -v git >/dev/null 2>&1 && git -C "$KIT_ROOT" rev-parse --git-dir >/dev/null 2>&1 || {
      kit_log "the writer's source is not next to the kit and the kit is not in a git checkout"; return 1; }
    kit_confirm "The record's writer is not next to the kit: fetch the branch $SILOREC_BRANCH from this repository's remote (a few MB) and unpack it into $KIT_WORK."
    git -C "$KIT_ROOT" fetch -q origin "$SILOREC_BRANCH" >&2 || { kit_log "git fetch of $SILOREC_BRANCH failed"; return 1; }
    rev="$(git -C "$KIT_ROOT" rev-parse FETCH_HEAD)" || return 1
    rm -rf "$KIT_WORK/src"; mkdir -p "$KIT_WORK/src"
    git -C "$(git -C "$KIT_ROOT" rev-parse --show-toplevel)" archive "$rev" research/2026-11-25-record-format/proto | tar -x -C "$KIT_WORK/src" || return 1
    kit_fact silorec_commit "$rev"
    src="$KIT_WORK/src/research/2026-11-25-record-format/proto"
  fi
  kit_fact silorec_source "$src"
  mkdir -p "$(dirname "$SILOREC_BIN")"
  # shellcheck disable=SC2086  # an empty $tflag is no argument
  if ! (cd "$src" && ZIG_GLOBAL_CACHE_DIR="$KIT_WORK/zig-cache" ZIG_LOCAL_CACHE_DIR="$KIT_WORK/zig-cache-local" \
          "$z" build --release=safe $tflag -p "$KIT_WORK/zig-out") > "$KIT_WORK/silorec-build.log" 2>&1; then
    kit_attach "$KIT_WORK/silorec-build.log"
    kit_log "zig build of silorec failed (see silorec-build.log)"
    return 1
  fi
  cp "$KIT_WORK/zig-out/bin/silorec" "$out" || return 1
  rm -rf "$KIT_WORK/zig-cache" "$KIT_WORK/zig-cache-local" "$KIT_WORK/zig-out"
}
