# Helpers for live checks (sourced by tools/agent/live and by the checks in live.d/).
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

# live_short_home: a short, per-run base directory (prints it). macOS refuses Unix socket paths of
# 104 bytes or more and $TMPDIR alone is ~50, so sockets must live under this, not under $STATE.
live_short_home() {
  local d="${LIVE_SHORT_BASE:-/tmp}/sl-$(printf %s "$STATE" | cksum | cut -d' ' -f1)"
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

# Engine identity: only the Dev engine of the bundle under test is ever used.
live_dev_sock() { echo "${DEV_SOCK:-$HOME/.silo-dev/engine.sock}"; }
live_assert_dev_sock() {
  case "$1" in
    "$HOME/.silo/"*|*/org.silo.app/*|*/Silo.app/*) live_die "refusing the production engine path: $1" ;;
  esac
  case "$1" in *.silo-dev*|"$STATE"/*|/tmp/sl-*) ;; *) live_die "not a Dev engine path: $1" ;; esac
}
# live_foreign_engines <silod>: pids of `silod --agent` processes that are not <silod> (never touched).
live_foreign_engines() {
  ps -axo pid=,command= | awk -v mine="$1 --agent" 'index($0, "silod --agent") && !index($0, mine) {print $1}'
}
# live_our_engine <silod>: pid of the engine started from <silod>, if any.
live_our_engine() { pgrep -f "^$1 --agent" | head -1; }

# Throwaway computers: e2e-<check>-<n>, one per line in $STATE/computers (read by `clean`).
live_computer() {
  local base="e2e-$(printf %s "$CHECK" | tr -c 'a-z0-9\n' '-' | sed 's/-*$//')" n=1
  touch "$STATE/computers"
  while grep -qx "$base-$n" "$STATE/computers"; do n=$((n+1)); done
  echo "$base-$n" | tee -a "$STATE/computers"
}
# live_computer_named <suffix>: e2e-<check>-<suffix>, recorded once.
live_computer_named() {
  local name="e2e-$(printf %s "$CHECK" | tr -c 'a-z0-9\n' '-' | sed 's/-*$//')-$1"
  touch "$STATE/computers"; grep -qx "$name" "$STATE/computers" || echo "$name" >>"$STATE/computers"
  echo "$name"
}
live_computers() { cat "$STATE/computers" 2>/dev/null; }

# Linux machine: reachable in 3 s or the run is "skipped: offline".
LIVE_LINUX_HOST=${LIVE_LINUX_HOST:-10.77.77.2}
live_linux_up() { ${LIVE_SSH:-ssh} -o BatchMode=yes -o ConnectTimeout=3 "$LIVE_LINUX_HOST" true >/dev/null 2>&1; }
