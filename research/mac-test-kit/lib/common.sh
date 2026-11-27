# Shared helpers for every test of the kit. Sourced, never run.
# Written for macOS /bin/bash 3.2 and BSD tools: no associative arrays, no mapfile, no ${x,,},
# no GNU-only flags. Timing and JSON go through python3, which the Command Line Tools install.

# Set by ./kit before a test starts:
#   KIT_ROOT     the kit's folder
#   KIT_ID       the test's id, two digits ("03")
#   KIT_WORK     a folder for this test only, under the kit's work folder; deleted by ./kit clean
#   KIT_OUT      the JSON-lines file this run writes its results into
#   KIT_YES      1 when the owner passed --yes: confirmations are answered yes, except sudo
#   KIT_SUDO     1 when the owner passed --allow-sudo: tests may run sudo after saying why
#   KIT_FAKE     1 when the kit runs on Linux for validation (./kit selftest): Mac-only commands
#                must not run; a test either runs its Linux analogue or stops with kit_skip

set -u

kit_now() { python3 -c 'import time; print("%.6f" % time.time())'; }

# kit_json_str "text" prints text as a JSON string.
kit_json_str() {
  python3 -c 'import json,sys; sys.stdout.write(json.dumps(sys.argv[1]))' "$1"
}

_kit_emit() { # _kit_emit kind json-fields-without-braces
  printf '{"test":"%s","kind":"%s","t":%s,%s}\n' "$KIT_ID" "$1" "$(kit_now)" "$2" >> "$KIT_OUT"
}

kit_log() { printf '[%s] %s\n' "$KIT_ID" "$*" >&2; }

# kit_note "text": a free-text observation kept in the results.
kit_note() { kit_log "$*"; _kit_emit note "\"text\":$(kit_json_str "$*")"; }

# kit_measure name value unit ["context"]: one number. value must be a JSON number.
kit_measure() {
  local ctx="${4:-}"
  kit_log "measure $1 = $2 $3 ${ctx}"
  _kit_emit measure "\"name\":$(kit_json_str "$1"),\"value\":$2,\"unit\":$(kit_json_str "$3"),\"context\":$(kit_json_str "$ctx")"
}

# kit_fact name "value": one observed string (an error code, a version, yes/no).
kit_fact() {
  kit_log "fact $1 = $2"
  _kit_emit fact "\"name\":$(kit_json_str "$1"),\"value\":$(kit_json_str "$2")"
}

# kit_attach path: keep a file (a log, a screenshot) next to the results. Copies it.
kit_attach() {
  local dst; dst="$(dirname "$KIT_OUT")/attachments/$KIT_ID"
  mkdir -p "$dst" && cp "$1" "$dst/" && _kit_emit attach "\"path\":$(kit_json_str "attachments/$KIT_ID/$(basename "$1")")"
}

# kit_outcome key "sentence": the test's answer. key must be one of the test's "# outcome" lines.
# A test calls it once, at the end. Calling it more than once keeps every call; the last one wins.
kit_outcome() {
  kit_log "OUTCOME $1: $2"
  _kit_emit outcome "\"key\":$(kit_json_str "$1"),\"text\":$(kit_json_str "$2")"
}

# kit_skip "reason": the test cannot run here. Exits the test.
kit_skip() { kit_log "SKIP: $*"; _kit_emit skip "\"reason\":$(kit_json_str "$*")"; exit 0; }

# kit_stop_run "why": the tests after this one must not run in this ./kit run (the person is asked
# to log out, say). The test itself goes on to its end.
kit_stop_run() {
  kit_log "the remaining tests of this run will not run: $*"
  [ -n "${KIT_STOP_FILE:-}" ] && echo "$*" > "$KIT_STOP_FILE"
  return 0
}

# kit_fail "reason": the test itself broke (not a finding). Exits the test with status 1.
kit_fail() { kit_log "ERROR: $*"; _kit_emit error "\"reason\":$(kit_json_str "$*")"; exit 1; }

kit_is_mac() { [ "$(uname -s)" = "Darwin" ]; }

# kit_need_mac [major]: skip unless this is an Apple silicon Mac on macOS >= major.
kit_need_mac() {
  kit_is_mac || kit_skip "needs a Mac"
  [ "$(uname -m)" = "arm64" ] || kit_skip "needs Apple silicon"
  if [ -n "${1:-}" ]; then
    local major; major="$(sw_vers -productVersion | cut -d. -f1)"
    [ "$major" -ge "$1" ] || kit_skip "needs macOS $1 or later, this Mac runs $(sw_vers -productVersion)"
  fi
}

# kit_need_cmd cmd "how to get it": skip when a command is missing.
kit_need_cmd() { command -v "$1" >/dev/null 2>&1 || kit_skip "needs $1: $2"; }

# kit_need_free_gb n: skip when the work folder's volume has less than n GB free.
kit_need_free_gb() {
  local free
  free="$(df -k "$KIT_WORK" | awk 'NR==2 {print int($4/1048576)}')"
  [ "$free" -ge "$1" ] || kit_skip "needs $1 GB free on the work folder's volume, $free GB free"
}

# kit_confirm "what will happen": ask before a long wait, a download or a VM. --yes answers it.
kit_confirm() {
  kit_log "ASK: $*"
  if [ "${KIT_YES:-0}" = 1 ]; then return 0; fi
  if [ ! -t 0 ]; then kit_skip "needs confirmation and no terminal is attached (pass --yes): $*"; fi
  printf '[%s] %s Continue? [y/N] ' "$KIT_ID" "$*" >&2
  local a; read -r a
  case "$a" in y|Y|yes) return 0 ;; *) kit_skip "declined: $*" ;; esac
}

# kit_sudo "why" cmd...: run one command as root. Needs --allow-sudo; says why first.
kit_sudo() {
  local why="$1"; shift
  [ "${KIT_SUDO:-0}" = 1 ] || kit_skip "needs sudo ($why); rerun with --allow-sudo"
  kit_log "SUDO ($why): $*"
  _kit_emit note "\"text\":$(kit_json_str "sudo: $why: $*")"
  sudo "$@"
}

# kit_human "instructions": a step only a person can do (a login in a browser). Waits for Enter.
# With --yes and no terminal, the test is skipped: nobody is there to do it.
kit_human() {
  kit_log "YOUR TURN: $*"
  if [ ! -t 0 ]; then kit_skip "needs a person at the keyboard: $*"; fi
  printf '\n[%s] YOUR TURN:\n%s\nPress Enter when done, or type s and Enter to skip. ' "$KIT_ID" "$*" >&2
  local a; read -r a
  [ "$a" = s ] && kit_skip "skipped by the person: $*"
  return 0
}

# kit_ask_yn "question": a yes/no observation only a person can make. Prints yes or no.
kit_ask_yn() {
  if [ ! -t 0 ]; then echo unknown; return 0; fi
  printf '[%s] %s [y/n] ' "$KIT_ID" "$1" >&2
  local a; read -r a
  case "$a" in y|Y|yes) echo yes ;; *) echo no ;; esac
}

# kit_time cmd...: run a command, print its wall time in seconds on stdout, its output to stderr.
kit_time() {
  local t0 t1 rc
  t0="$(kit_now)"; "$@" >&2; rc=$?; t1="$(kit_now)"
  python3 -c "print('%.3f' % ($t1 - $t0))"
  return $rc
}

# The VM helper, built by ./kit build into the work folder. See swift/README.md.
SILOVM="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin/silovm"
kit_need_silovm() {
  [ -x "$SILOVM" ] || kit_skip "the VM helper is not built: run ./kit build"
}

# The installed, never started macOS computer every VM test clones from. Made by test 01's
# install step (./kit run 00); see tests/00-install-base.sh.
KIT_BASE_VM="${KIT_WORK_ROOT:-$HOME/SiloKit}/base/installed.silovm"
kit_need_base_vm() {
  [ -d "$KIT_BASE_VM" ] || kit_skip "no installed macOS computer yet: run ./kit run 00 first (download and install, about an hour)"
}
