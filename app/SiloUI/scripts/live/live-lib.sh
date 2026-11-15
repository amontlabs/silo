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
live_register_tempdir() { grep -qxF -- "$1" "$STATE/tempdirs" 2>/dev/null || echo "$1" >>"$STATE/tempdirs"; }

# live_strip_slashes <path>: <path> without trailing slashes (/ stays /).
live_strip_slashes() { local p=$1; while [ "${#p}" -gt 1 ] && [ "${p%/}" != "$p" ]; do p=${p%/}; done; echo "$p"; }
# Where short per-run directories go: LIVE_SHORT_BASE, else ATELIER_TMP (`atelier tmp --short` makes it short), else /tmp.
# Fails (with a message) unless it is an absolute path.
live_short_base() {
  local b=${LIVE_SHORT_BASE:-${ATELIER_TMP:-/tmp}}
  case "$b" in /*) ;; *) echo "live: the short base must be an absolute path (LIVE_SHORT_BASE, ATELIER_TMP): $b" >&2; return 1;; esac
  live_strip_slashes "$b"
}

# live_short_home: a short, per-run base directory (prints it). macOS refuses Unix socket paths of
# 104 bytes or more and $TMPDIR alone is ~50, so sockets must live under this, not under $STATE.
live_short_home() {
  local b d; b=$(live_short_base) || live_die "no usable short base"
  d="$b/sl-$(printf %s "$STATE" | cksum | cut -d' ' -f1)"
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
# LIVE_ENGINE=own (set by the runner for a check with CHECK_OWN_ENGINE): the run's own Dev engine instance, whose
# HOME is a short per-state-dir folder; silod keeps everything it owns under that HOME (state dir and socket in
# .silo-dev, data, cache and logs under Library/, every computer's runtime home), so two instances share none of it.
# Otherwise: the person's Dev engine under the real HOME.
live_engine_own() { [ "${LIVE_ENGINE:-}" = own ]; }
# The own instance's HOME is a folder of its own right under the short base (made once per state dir, recorded in
# $STATE/engine-home): silod refuses a HOME longer than LIVE_ENGINE_HOME_MAX bytes, because every computer's
# runtime socket (<HOME>/.silo-dev/<12 hex>/run/sandboxes/<24 chars>/control.sock) must stay under 104 bytes.
LIVE_ENGINE_HOME_MAX=28
live_engine_home() {
  live_engine_own || { echo "$HOME"; return 0; }
  local h b; h=$(live_strip_slashes "$(cat "$STATE/engine-home" 2>/dev/null)")
  if [ -z "$h" ] || [ ! -d "$h" ]; then
    b=$(live_short_base) || live_die "no usable short base"
    h=$(mktemp -d "$b/sl-XXX") || live_die "cannot make an engine home under $b"
    echo "$h" >"$STATE/engine-home"; live_register_tempdir "$h"; live_leftover_put "$(live_engine_home_line "$h")"
  fi
  echo "$h"
}
live_dev_sock() {
  if live_engine_own; then echo "$(live_engine_home)/.silo-dev/engine.sock"; else echo "${DEV_SOCK:-$HOME/.silo-dev/engine.sock}"; fi
}
# The Dev engine's application data folder (guest images, configuration lock, saved settings).
live_engine_data() { echo "$(live_engine_home)/Library/Application Support/org.silo.dev"; }
# live_engine_prepare: makes the own instance's HOME and gives it the Dev guest images as an APFS clone of the
# person's copy (cp -c: no extra space, and nothing the instance writes reaches the original). No-op when shared.
live_engine_prepare() {
  live_engine_own || return 0
  local h src dst why; h=$(live_engine_home) || exit 1; src="$HOME/Library/Application Support/org.silo.dev/guest-image"; dst="$(live_engine_data)/guest-image"
  why=$(live_engine_home_check "$h") || live_die "refusing the own engine home: $why"
  [ ${#h} -le $LIVE_ENGINE_HOME_MAX ] || live_die "engine home $h is longer than $LIVE_ENGINE_HOME_MAX bytes: use a shorter base (atelier tmp --short, LIVE_SHORT_BASE)"
  mkdir -p "$h" && chmod 700 "$h" || live_die "cannot make $h"
  [ -e "$dst" ] && return 0
  live_need_path "$src" "Dev guest images to clone"
  mkdir -p "$(dirname "$dst")" && cp -cR "$src" "$dst.tmp.$$" && mv "$dst.tmp.$$" "$dst" || live_die "cannot clone $src"
}
# live_rm_tree <dir>: removes <dir> even when parts of it are read-only (silod keeps the guest images read-only, so
# plain rm -rf fails inside them): makes it writable first, never following links.
live_rm_tree() {
  [ -e "$1" ] || [ -L "$1" ] || return 0
  [ -L "$1" ] || chmod -R u+w "$1" 2>/dev/null
  rm -rf "$1" && [ ! -e "$1" ]
}
# live_engine_home_line <home>: the $ATELIER_TMP/leftovers line of an own engine home. `atelier clean` cannot remove
# its read-only guest images, so the line stays until the harness has removed the home (cleanup or `run sweep`).
# The line names the canonical state dir, so every spelling of it (a link, a trailing slash) gives the same line.
live_engine_home_line() { local st; st=$(live_canon "$STATE"); echo "engine home $1 (live check state $st; remove with: run sweep $st)"; }
# live_engine_home_check <home>: whether <home> is an own engine home that may be prepared or removed: a plain
# absolute path named sl-*, directly under the short base (both resolved), not itself a link, outside HOME, and (when
# it exists) a folder of this user. Prints why not and fails otherwise. The recorded path is kept as made (not
# resolved): silod limits the HOME length, and resolving /tmp adds /private.
live_engine_home_check() {
  local b; b=$(live_short_base) || { echo "no usable short base"; return 1; }
  python3 -I -c 'import os,stat,sys
h,b,home=sys.argv[1:4]
def no(m):
    print(m); sys.exit(1)
if not h.startswith("/") or os.path.normpath(h)!=h: no("not a plain absolute path: "+h)
n=os.path.basename(h)
if not n.startswith("sl-"): no("not named sl-*: "+h)
cb=os.path.realpath(b)
if os.path.realpath(os.path.dirname(h))!=cb: no("not directly under the short base %s: %s (sweep with LIVE_SHORT_BASE set to its folder)"%(b,h))
ch=os.path.realpath(h)
if ch!=os.path.join(cb,n): no("a link, not a folder: "+h)
rh=os.path.realpath(home)
if ch==rh or ch.startswith(rh+"/"): no("inside HOME: "+h)
if os.path.lexists(h):
    st=os.lstat(h)
    if not stat.S_ISDIR(st.st_mode) or st.st_uid!=os.geteuid(): no("not a folder of this user: "+h)' "$1" "$b" "$HOME"
}
# live_engine_remove: deletes the own instance's HOME (recorded in $STATE/engine-home) once its engine has stopped
# (call from cleanup when no computer is left, or through `run sweep`). Works whatever LIVE_ENGINE is now: the
# record alone says there is a home.
live_engine_remove() {
  local raw h why; raw=$(cat "$STATE/engine-home" 2>/dev/null); h=$(live_strip_slashes "$raw"); [ -n "$h" ] || return 0
  if [ -e "$h" ] || [ -L "$h" ]; then
    why=$(live_engine_home_check "$h") || live_die "not an own engine home, left in place: $why"
    [ -d "$h" ] && [ ! -L "$h" ] && [ -O "$h" ] || live_die "not an own engine home, left in place: $h"
    live_rm_tree "$h" || live_die "cannot remove the engine home $h"
  fi
  live_leftover_drop "$(live_engine_home_line "$h")"; [ "$raw" = "$h" ] || live_leftover_drop "$(live_engine_home_line "$raw")"
  rm -f "$STATE/engine-home"
}
# live_engine_answers <socket>: whether an engine accepts connections on <socket>.
live_engine_answers() {
  python3 -I -c 'import socket,sys
s=socket.socket(socket.AF_UNIX); s.settimeout(2)
try: s.connect(sys.argv[1])
except OSError: sys.exit(1)' "$1" 2>/dev/null
}
# live_lock_held <file>: whether another open file holds an flock on <file> (silod holds its .silo-dev/engine.lock
# from start to exit, before its socket answers). Takes the lock only for an instant, never creates the file.
live_lock_held() {
  python3 -I -c 'import fcntl,os,sys
try: f=os.open(sys.argv[1],os.O_RDONLY|os.O_NOFOLLOW)
except OSError: sys.exit(1)
try: fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)
except OSError: sys.exit(0)
sys.exit(1)' "$1" 2>/dev/null
}
# live_engine_alive <state> <home>: whether the own engine of <home> may still run (its socket answers, the silod
# --agent recorded in <state>/engine.pid still runs, or its engine.lock is held). Prints why and how to stop it.
live_engine_alive() {
  local st=$1 h=$2 sock=$2/.silo-dev/engine.sock lock=$2/.silo-dev/engine.lock rp="" pid stop
  rp=$(cat "$st/engine.pid" 2>/dev/null)
  case "$rp" in ''|*[!0-9]*) rp="";; *) case "$(ps -o command= -p "$rp" 2>/dev/null)" in *silod" --agent"*) ;; *) rp="";; esac;; esac
  pid=$rp; [ -n "$pid" ] || { [ -e "$lock" ] && pid=$(lsof -t -- "$lock" 2>/dev/null | sort -u | tr '\n' ' '); pid=${pid% }; }
  if [ -n "$pid" ]; then stop="stop it with: kill -TERM $pid, then rerun: run sweep $st"
  else stop="stop the silod --agent whose HOME is $h, then rerun: run sweep $st"; fi
  if live_engine_answers "$sock"; then echo "its engine still answers on $sock (pid ${pid:-unknown}); $stop"; return 0; fi
  if [ -n "$rp" ]; then echo "its engine (pid $rp) still runs, its socket does not answer yet; $stop"; return 0; fi
  if live_lock_held "$lock"; then echo "an engine holds $lock (pid ${pid:-unknown}); $stop"; return 0; fi
  return 1
}
# A client of an own instance (the silo CLI) runs with HOME=$(live_engine_home) too: the CLI accepts only the Dev
# socket under its HOME's .silo-dev.
# live_engine_launch <silod> [args]: starts <silod> --agent in the background (prints nothing; the caller records
# $!). An own instance gets its HOME and an ephemeral website-proxy port, so it never takes the Dev engine's.
live_engine_launch() {
  local silod=$1; shift
  if live_engine_own; then
    live_engine_prepare
    HOME=$(live_engine_home) SILO_URL_PROXY_PORT=0 exec "$silod" --agent "$@"
  else exec "$silod" --agent "$@"; fi
}
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
  local c h st sb; sb=$(live_short_base) || live_die "no usable short base"; sb=$(live_canon "$sb")
  c=$(live_canon "$1"); h=$(live_canon "$HOME"); st=$(live_canon "$STATE")
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
live_leftover_add() { live_leftover_put "$(live_leftover_line "$1")"; }
live_leftover_remove() { live_leftover_drop "$(live_leftover_line "$1")"; }
# live_leftover_put / live_leftover_drop <line>: adds a line once / removes it.
live_leftover_put() {
  [ -n "${ATELIER_TMP:-}" ] || return 0
  live_leftover_drop "$1"; echo "$1" >>"$ATELIER_TMP/leftovers"
}
live_leftover_drop() {
  [ -n "${ATELIER_TMP:-}" ] && [ -f "$ATELIER_TMP/leftovers" ] || return 0
  local f=$ATELIER_TMP/leftovers
  grep -vxF -- "$1" "$f" >"$f.$$.tmp" || true
  mv "$f.$$.tmp" "$f"
}
# Computers and an own engine home a resumed state dir still lists must be in the (possibly new) folder's
# leftovers file too.
live_sync_leftovers() {
  [ -n "${ATELIER_TMP:-}" ] || return 0
  local c h; for c in $(live_computers); do live_leftover_add "$c"; done
  h=$(live_strip_slashes "$(cat "$STATE/engine-home" 2>/dev/null)"); [ -n "$h" ] && [ -e "$h" ] && live_leftover_put "$(live_engine_home_line "$h")"
  return 0
}

# Linux machine: reachable in 3 s or the run is "skipped: offline".
LIVE_LINUX_HOST=${LIVE_LINUX_HOST:-10.77.77.2}
live_linux_up() { ${LIVE_SSH:-ssh} -o BatchMode=yes -o ConnectTimeout=3 "$LIVE_LINUX_HOST" true >/dev/null 2>&1; }
