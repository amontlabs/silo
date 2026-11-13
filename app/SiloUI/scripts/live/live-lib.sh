# Helpers for live checks (sourced by app/SiloUI/scripts/live/run and by the checks in live.d/).
# Inside a step these are available: STATE, LOGS, CHECK, HOST, REPO, LIVE_STEP.

live_say() { echo "[$(date +%H:%M:%S)] $*"; }
live_die() { echo "[$(date +%H:%M:%S)] FATAL $*" >&2; exit 1; }

# live_check <name> <rc> [detail]: prints PASS/FAIL; a FAIL makes the step fail once it has ended.
live_check() {
  local name=$1 rc=$2; shift 2
  if [ "$rc" = 0 ]; then live_say "PASS  $name  -- $*"
  else live_say "FAIL  $name  -- $*"; : >"$STATE/steps/$LIVE_STEP.failed"; fi
}

# live_skip <reason>: ends the step and makes the whole run "skipped".
live_skip() { echo "$*" >"$STATE/skip-reason"; exit 77; }

# Values that must survive from one step to a later one (steps run in subshells).
live_set() { mkdir -p "$STATE/vars"; printf '%s' "$2" >"$STATE/vars/$1"; }
live_get() { cat "$STATE/vars/$1" 2>/dev/null; }

live_need_exec() { [ -x "$1" ] || live_die "missing executable: $1${2:+ ($2)}"; }
live_need_path() { [ -e "$1" ] || live_die "missing path: $1${2:+ ($2)}"; }

# live_register_tempdir <dir>: tells `clean` which directories belong to this run.
live_register_tempdir() { echo "$1" >>"$STATE/tempdirs"; }

# Where short per-run directories go: LIVE_SHORT_BASE, else ATELIER_TMP (`atelier tmp --short` makes it short), else /tmp.
live_short_base() { echo "${LIVE_SHORT_BASE:-${ATELIER_TMP:-/tmp}}"; }

# live_short_home: a short, per-run base directory (prints it). macOS refuses Unix socket paths of
# 104 bytes or more and $TMPDIR alone is ~50, so sockets must live under this, not under $STATE.
live_short_home() {
  local d; d="$(live_short_base)/sl-$(printf %s "$STATE" | cksum | cut -d' ' -f1)"
  mkdir -p "$d" && live_register_tempdir "$d" && echo "$d"
}

# live_check_sock_path <path>: fails fast when a socket at <path> cannot be bound (limit 103 bytes).
live_check_sock_path() {
  local n; n=$(printf %s "$1" | LC_ALL=C wc -c | tr -d ' ')
  [ "$n" -lt 104 ] || live_die "socket path is $n bytes (limit 103): $1 -- use live_short_home"
}

# live_link <target> <link>: makes <link> a symlink to an existing <target> (guest-image folder,
# runtime folders). Replaces nothing that is not already such a link.
live_link() {
  local target=$1 link=$2
  [ -e "$target" ] || live_die "link target missing: $target"
  if [ -L "$link" ]; then
    [ "$(readlink "$link")" = "$target" ] || live_die "$link points to $(readlink "$link"), expected $target"
  elif [ -e "$link" ]; then live_die "$link exists and is not a symlink"
  else mkdir -p "$(dirname "$link")" && ln -s "$target" "$link"; fi
}

# live_canon <path>: absolute path with symlinks and .. resolved (the path need not exist).
live_canon() { python3 -I -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$1"; }
# live_fileid <file>: size and mtime of the resolved file (changes when a binary is rebuilt).
live_fileid() { python3 -I -c 'import os,sys; s=os.stat(sys.argv[1]); print(s.st_size, s.st_mtime_ns)' "$1" 2>/dev/null || echo missing; }

# Engine identity: only the Dev engine of the bundle under test is ever used.
live_dev_sock() { echo "${DEV_SOCK:-$HOME/.silo-dev/engine.sock}"; }
# Refuses anything that resolves under ~/.silo or inside a production bundle.
live_is_production() {
  local c h; c=$(live_canon "$1"); h=$(live_canon "$HOME")
  case "$c" in "$h/.silo"|"$h/.silo/"*|*/org.silo.app|*/org.silo.app/*|*/Silo.app/*|/Applications/Silo*) return 0;; esac
  return 1
}
# live_assert_dev_engine <path>...: binaries of the engine under test must not be the production ones.
live_assert_dev_engine() {
  local p; for p in "$@"; do live_is_production "$p" && live_die "refusing a production engine binary: $p ($(live_canon "$p"))"; done; return 0
}
live_assert_dev_sock() {
  live_is_production "$1" && live_die "refusing the production engine path: $1 ($(live_canon "$1"))"
  local c h st sb; c=$(live_canon "$1"); h=$(live_canon "$HOME"); st=$(live_canon "$STATE"); sb=$(live_canon "$(live_short_base)")
  case "$c" in "$h/.silo-dev/"*|"$st/"*|"$sb/sl-"*) ;; *) live_die "not a Dev engine path: $1 ($c)" ;; esac
}
# live_foreign_engines <silod>: pids of `silod --agent` processes that are not <silod> (never touched).
live_foreign_engines() {
  ps -axo pid=,command= | awk -v mine="$1 --agent" 'index($0, "silod --agent") && !index($0, mine) {print $1}'
}
# The only engine a check may stop or kill is one it launched: record its pid right after launching.
live_engine_record() { echo "$1" >"$STATE/engine.pid"; }
# live_our_engine <silod>: pid of the engine this run launched, if it is still that process.
live_our_engine() {
  local pid cmd; pid=$(cat "$STATE/engine.pid" 2>/dev/null); [ -n "$pid" ] || return 0
  cmd=$(ps -o command= -p "$pid" 2>/dev/null)
  case "$cmd" in "$1 --agent"*) echo "$pid";; esac
}

# Short id of this run's resources, kept with the stamps (dropped when they are).
live_runid() {
  [ -s "$STATE/runid" ] || { mkdir -p "$STATE"; LC_ALL=C tr -dc 'a-f0-9' </dev/urandom | head -c 6 >"$STATE/runid"; }
  cat "$STATE/runid"
}
# Throwaway computers: e2e-<check>-<runid>-<n>, one per line in $STATE/computers (read by `clean`).
live_computer_base() { echo "e2e-$(printf %s "$CHECK" | tr -c 'a-z0-9\n' '-' | sed 's/-*$//')-$(live_runid)"; }
live_computer() {
  local base n=1; base=$(live_computer_base)
  touch "$STATE/computers"
  while grep -qx "$base-$n" "$STATE/computers"; do n=$((n+1)); done
  echo "$base-$n" >>"$STATE/computers"; live_leftover_add "$base-$n"
  echo "$base-$n"
}
# live_computer_named <suffix>: e2e-<check>-<runid>-<suffix>, recorded once (when first used).
live_computer_named() {
  local name; name="$(live_computer_base)-$1"
  touch "$STATE/computers"
  grep -qx "$name" "$STATE/computers" || { echo "$name" >>"$STATE/computers"; live_leftover_add "$name"; }
  echo "$name"
}
live_computers() { cat "$STATE/computers" 2>/dev/null; }
# live_forget_computer <name>: call from cleanup() once the computer is really gone.
live_forget_computer() {
  grep -vxF -- "$1" "$STATE/computers" >"$STATE/computers.tmp" 2>/dev/null; mv "$STATE/computers.tmp" "$STATE/computers"
  live_leftover_remove "$1"
}

# Outside resources under `atelier tmp`: one line per throwaway computer in $ATELIER_TMP/leftovers (the folder's
# contract: `atelier clean` refuses while the file is not empty), removed once the computer is gone. No-ops
# without ATELIER_TMP.
live_leftover_line() { echo "computer $1 (live check ${CHECK:-?}, Dev engine, state $STATE)"; }
live_leftover_add() {
  [ -n "${ATELIER_TMP:-}" ] || return 0
  live_leftover_remove "$1"; live_leftover_line "$1" >>"$ATELIER_TMP/leftovers"
}
live_leftover_remove() {
  [ -n "${ATELIER_TMP:-}" ] && [ -f "$ATELIER_TMP/leftovers" ] || return 0
  local f=$ATELIER_TMP/leftovers
  grep -vxF -- "$(live_leftover_line "$1")" "$f" >"$f.$$.tmp" || true
  mv "$f.$$.tmp" "$f"
}
# Computers a resumed state dir still lists must be in the (possibly new) folder's leftovers file too.
live_sync_leftovers() {
  [ -n "${ATELIER_TMP:-}" ] || return 0
  local c; for c in $(live_computers); do live_leftover_add "$c"; done
}

# Linux machine: reachable in 3 s or the run is "skipped: offline".
LIVE_LINUX_HOST=${LIVE_LINUX_HOST:-10.77.77.2}
live_linux_up() { ${LIVE_SSH:-ssh} -o BatchMode=yes -o ConnectTimeout=3 "$LIVE_LINUX_HOST" true >/dev/null 2>&1; }
