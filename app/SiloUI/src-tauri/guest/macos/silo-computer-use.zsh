#!/bin/zsh
# Computer use for a Silo macOS guest: the official ChatGPT app, LCU's macOS build and the
# TCC grants that stand in for the "Allow" dialogs.
#
# Silo copies this script, pinned.json and the archives named there into one folder in the
# guest and runs it as the working account (`silo`, with passwordless sudo and System Integrity
# Protection off):
#
#   silo-computer-use.zsh apply --approval ask|auto   install, grant, register; idempotent
#   silo-computer-use.zsh app-present                 exit 0 when the pinned app is installed
#   silo-computer-use.zsh status                      print the receipt
#   silo-computer-use.zsh grant SERVICE CLIENT PATH   write one TCC row (used by tests)
#
# `apply` steps:
#   1. check that System Integrity Protection is off;
#   2. install /Applications/ChatGPT.app from the staged zip when the pinned version is not
#      there: SHA-256, code signature (`codesign --verify --deep --strict`), bundle identifier
#      and team are checked, then the app is moved in and its quarantine attribute cleared;
#   3. install LCU's runtime against that app (`scripts/install.sh --runtime-only --yes`);
#   4. grant Accessibility and Screen Recording to the app and its Computer Use helper in the
#      system TCC database, and pre-write the Screen Recording reminder ledger;
#   5. register every agent (`lcu setup --agent all --allow-missing ...`) and install a
#      LaunchAgent that runs `lcu setup --reconcile` at each login.
# Per-app approvals ("Allow Computer Use to use X?") stay with LCU and are never seeded.
#
# The TCC row layout and csreq derivation follow trycua/cua `seed-tcc.sh` and actions/runner-images
# `configure-tccdb-macos.sh` (both MIT); the Screen Recording ledger follows electron/electron's
# `screencapture-nag-remover.sh` (MIT).
#
# Output: progress lines on stdout, `error: <reason>` on stderr and exit 1 on failure. The full
# log is ~/Library/Logs/silo-computer-use.log; the receipt is
# ~/Library/Application Support/Silo/computer-use-receipt.json.

set -u
set -o pipefail
export PATH=/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin

STAGE=${0:A:h}
PINNED=$STAGE/pinned.json
LOG=$HOME/Library/Logs/silo-computer-use.log
RECEIPT_DIR=$HOME/Library/Application\ Support/Silo
RECEIPT=$RECEIPT_DIR/computer-use-receipt.json
WORK=${SILO_CU_WORK:-$STAGE/work}
APP=/Applications/ChatGPT.app
HELPER_REL="Contents/Resources/cua_node/lib/node_modules/@oai/sky/Codex Computer Use.app"
LCU=$HOME/.local/share/lcu/current/bin/lcu
TCC_DB=${SILO_CU_TCC_DB:-/Library/Application Support/com.apple.TCC/TCC.db}
SUDO=${SILO_CU_SUDO-sudo -n}
AGENT_LABEL=org.silo.computer-use-reconcile
AGENT_PLIST=$HOME/Library/LaunchAgents/$AGENT_LABEL.plist
LEDGER=$HOME/Library/Group\ Containers/group.com.apple.replayd/ScreenCaptureApprovals.plist
SERVICES=(kTCCServiceAccessibility kTCCServiceScreenCapture)
APPROVAL=ask

mkdir -p "$HOME/Library/Logs" "$RECEIPT_DIR"

log() { print -r -- "$(date '+%Y-%m-%d %H:%M:%S') $*" >>"$LOG"; }
say() { log "$*"; print -r -- "$*"; }

# Runs a command with its output appended to the log.
logged() {
  log "\$ $*"
  "$@" >>"$LOG" 2>&1
  local result=$?
  log "exit $result"
  return $result
}

pinned() { plutil -extract "$1" raw -o - "$PINNED" 2>/dev/null; }

write_receipt() { # state reason
  local app lcu
  app=$(pinned chatgptVersion) lcu=$(pinned lcuVersion)
  printf '{"schemaVersion":1,"state":"%s","reason":"%s","chatgptVersion":"%s","lcuVersion":"%s","approval":"%s","at":"%s"}\n' \
    "$1" "${2//[^A-Za-z0-9 .,:_-]/}" "$app" "$lcu" "$APPROVAL" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >"$RECEIPT.tmp" &&
    mv -f "$RECEIPT.tmp" "$RECEIPT"
}

fail() {
  log "error: $*"
  print -u2 -r -- "error: $*"
  write_receipt failed "$*"
  exit 1
}

sha256_of() { shasum -a 256 "$1" | cut -d' ' -f1; }

check_sha() { # file expected label
  [[ -f $1 ]] || fail "$3 was not copied to the computer"
  [[ $(sha256_of "$1") == "$2" ]] || fail "$3 does not match its pinned SHA-256"
}

# MARK: SIP

sip_disabled() { [[ $(csrutil status 2>/dev/null) == *"status: disabled"* ]]; }

# MARK: Archives

# Fails unless every member name in the list on stdin is relative and has no `..` component.
safe_members() {
  local name
  while IFS= read -r name; do
    [[ -n $name && $name != /* && $name != ".." && $name != ../* && $name != */../* && $name != */.. ]] || return 1
  done
}

# Fails when a symbolic link under $1 does not resolve to a path inside $1; a dangling link fails.
links_stay_inside() {
  local root=${1:A} link target
  find "$root" -type l -print0 | while IFS= read -r -d '' link; do
    [[ -e $link ]] || return 1
    target=${link:A}
    [[ $target == $root || $target == $root/* ]] || return 1
  done
}

# MARK: ChatGPT app

app_info() { /usr/libexec/PlistBuddy -c "Print :$2" "$1/Contents/Info.plist" 2>/dev/null; }

# Whether the bundle at $1 is the pinned, officially signed app.
app_ok() {
  local code=$1 team
  [[ -d $code ]] || return 1
  [[ $(app_info "$code" CFBundleIdentifier) == "$(pinned bundleIdentifier)" ]] || return 1
  [[ $(app_info "$code" CFBundleShortVersionString) == "$(pinned chatgptVersion)" ]] || return 1
  team=$(codesign -dv "$code" 2>&1 | sed -n 's/^TeamIdentifier=//p')
  [[ $team == "$(pinned teamIdentifier)" ]] || return 1
  codesign --verify --deep --strict "$code" >>"$LOG" 2>&1
}

install_app() {
  if app_ok "$APP"; then
    say "ChatGPT $(pinned chatgptVersion) is already installed"
    return 0
  fi
  say "Installing ChatGPT $(pinned chatgptVersion)"
  local zip=$STAGE/$(pinned chatgptArchive)
  check_sha "$zip" "$(pinned chatgptSha256)" "The ChatGPT app archive"
  rm -rf "$WORK/app" && mkdir -p "$WORK/app" || fail "could not prepare the ChatGPT app folder"
  zipinfo -1 "$zip" 2>>"$LOG" | safe_members || fail "the ChatGPT app archive has unsafe paths"
  logged ditto -x -k "$zip" "$WORK/app" || fail "could not unpack the ChatGPT app"
  [[ $(ls -A "$WORK/app") == ChatGPT.app ]] || fail "the ChatGPT app archive holds more than the app"
  links_stay_inside "$WORK/app/ChatGPT.app" || fail "the ChatGPT app archive has links that leave the app"
  app_ok "$WORK/app/ChatGPT.app" || fail "the ChatGPT app is not the official signed app"
  # LCU and the Computer Use helper run from /Applications, owned by root.
  if [[ -e $APP ]]; then
    logged ${=SUDO} rm -rf "$APP" || fail "could not remove the previous ChatGPT app"
  fi
  logged ${=SUDO} mv "$WORK/app/ChatGPT.app" "$APP" || fail "could not move the ChatGPT app into Applications"
  logged ${=SUDO} chown -R root:wheel "$APP" || fail "could not hand the ChatGPT app to root"
  ${=SUDO} xattr -dr com.apple.quarantine "$APP" >>"$LOG" 2>&1
  app_ok "$APP" || fail "the installed ChatGPT app did not verify"
  rm -rf "$WORK/app" "$zip"
}

# MARK: LCU

lcu_current() { # whether the installed LCU is the pinned one for the installed app
  [[ -x $LCU ]] || return 1
  local version
  version=$("$LCU" --version 2>/dev/null) || return 1
  [[ $version == "lcu $(pinned lcuVersion) (ChatGPT darwin $(pinned chatgptVersion);"* ]]
}

install_lcu() {
  if lcu_current; then
    say "LCU $(pinned lcuVersion) is already installed"
    return 0
  fi
  say "Installing LCU $(pinned lcuVersion)"
  local archive=$STAGE/$(pinned lcuArchive)
  check_sha "$archive" "$(pinned lcuSha256)" "The LCU archive"
  rm -rf "$WORK/lcu" && mkdir -p "$WORK/lcu" || fail "could not prepare the LCU folder"
  tar -tzf "$archive" 2>>"$LOG" | safe_members || fail "the LCU archive has unsafe paths"
  logged tar -xzf "$archive" -C "$WORK/lcu" || fail "could not unpack LCU"
  links_stay_inside "$WORK/lcu" || fail "the LCU archive has links that leave its folder"
  local release=$WORK/lcu/${$(pinned lcuArchive)%.tar.gz}
  [[ -x $release/scripts/install.sh ]] || fail "the LCU archive has no installer"
  # The installer verifies the app's signature under a time limit; the first verification after
  # the app was installed can be slower than that, and the second finds the files cached.
  (cd "$release" && { logged ./scripts/install.sh --runtime-only --yes || logged ./scripts/install.sh --runtime-only --yes; }) ||
    fail "the LCU installer failed"
  lcu_current || fail "LCU did not install the pinned version"
  rm -rf "$WORK/lcu" "$archive"
}

# MARK: TCC

# The designated requirement of the code at $1 as a csreq blob in hex.
csreq_hex() {
  local requirement=$WORK/requirement.txt blob=$WORK/requirement.bin
  codesign -d -r- "$1" 2>&1 | sed -n 's/^# *designated => //p; s/^designated => //p' >"$requirement"
  [[ -s $requirement ]] || return 1
  rm -f "$blob"
  csreq -r "$requirement" -b "$blob" >>"$LOG" 2>&1 || return 1
  xxd -p "$blob" | tr -d '\n'
}

# Writes one row of the system TCC database for the code at $3. Only columns that exist in this
# macOS version's `access` table are written; the others keep their defaults.
grant() { # service client code
  local service=$1 client=$2 code=$3 hex columns
  hex=$(csreq_hex "$code") || { log "no designated requirement for $code"; return 1; }
  columns=("${(@f)$(${=SUDO} sqlite3 "$TCC_DB" "select name from pragma_table_info('access');")}")
  local -A values=(
    service "'$service'" client "'$client'" client_type 0 auth_value 2 auth_reason 4 auth_version 1
    csreq "X'$hex'" indirect_object_identifier "'UNUSED'" flags 0
    last_modified "strftime('%s','now')" last_reminded "strftime('%s','now')"
  )
  local name names=() chosen=()
  for name in service client auth_value csreq; do
    (( ${columns[(Ie)$name]} )) || { log "the TCC access table has no $name column"; return 1; }
  done
  for name in $columns; do
    if (( ${+values[$name]} )); then
      names+=($name)
      chosen+=($values[$name])
    fi
  done
  ${=SUDO} sqlite3 "$TCC_DB" "insert or replace into access (${(j:,:)names}) values (${(j:,:)chosen});" >>"$LOG" 2>&1
}

granted() { # service client
  local rows
  rows=$(${=SUDO} sqlite3 "$TCC_DB" "select count(*) from access where service='$1' and client='$2' and client_type=0 and auth_value=2 and csreq is not null;" 2>/dev/null)
  [[ $rows == 1 ]]
}

grant_clients() {
  local helper=$APP/$HELPER_REL code client service
  local clients=("$APP" "$helper")
  for code in $clients; do
    client=$(app_info "$code" CFBundleIdentifier)
    [[ -n $client ]] || fail "could not read the identifier of ${code:t}"
    for service in $SERVICES; do
      grant "$service" "$client" "$code" || fail "could not write the $service grant for $client"
    done
  done
  for code in $clients; do
    client=$(app_info "$code" CFBundleIdentifier)
    for service in $SERVICES; do
      granted "$service" "$client" || fail "the $service grant for $client did not read back"
    done
  done
}

# Screen Recording on macOS 15 and later asks again after a while; a far-future date in the
# approvals ledger of replayd stops it. 15.1 and later key it by bundle identifier, earlier
# releases by executable path.
write_ledger() {
  local version major minor future code client
  version=$(sw_vers -productVersion)
  major=${version%%.*} minor=${${version#*.}%%.*}
  (( major >= 15 )) || return 0
  future=$(date -u -v+100y '+%Y-%m-%d %H:%M:%S +0000')
  mkdir -p "${LEDGER:h}" || return 1
  for code in "$APP" "$APP/$HELPER_REL"; do
    if (( major > 15 || minor >= 1 )); then
      client=$(app_info "$code" CFBundleIdentifier)
      defaults write "$LEDGER" "$client" -dict \
        kScreenCaptureApprovalLastAlerted -date "$future" \
        kScreenCaptureApprovalLastUsed -date "$future" \
        kScreenCapturePrivacyHintDate -date "$future" \
        kScreenCapturePrivacyHintPolicy -int 3153600000 \
        kScreenCaptureAlertableUsageCount -int 0 || return 1
    else
      defaults write "$LEDGER" "$(app_executable "$code")" -date "$future" || return 1
    fi
  done
  killall -u "$USER" cfprefsd >/dev/null 2>&1
  return 0
}

app_executable() { print -r -- "$1/Contents/MacOS/$(app_info "$1" CFBundleExecutable)"; }

restart_tccd() {
  launchctl kickstart -k "gui/$(id -u)/com.apple.tccd" >>"$LOG" 2>&1
  ${=SUDO} launchctl kickstart -k system/com.apple.tccd.system >>"$LOG" 2>&1 ||
    log "the system tccd was not restarted; the grants apply after the next boot"
}

# MARK: Registration

register() {
  say "Registering LCU with every agent"
  logged "$LCU" setup --agent all --allow-missing --session direct --yes --approval "$APPROVAL" ||
    fail "lcu setup failed"
  install_agent
}

install_agent() {
  mkdir -p "${AGENT_PLIST:h}" || fail "could not create the LaunchAgents folder"
  cat >"$AGENT_PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>$AGENT_LABEL</string>
<key>ProgramArguments</key><array><string>$LCU</string><string>setup</string><string>--reconcile</string></array>
<key>RunAtLoad</key><true/>
</dict></plist>
EOF
  [[ $? -eq 0 && -s $AGENT_PLIST ]] || fail "could not write the reconcile LaunchAgent"
  plutil -lint "$AGENT_PLIST" >>"$LOG" 2>&1 || fail "the reconcile LaunchAgent is not a valid property list"
  # Without a login session yet, bootstrap fails; launchd loads the agent at the next login.
  launchctl bootout "gui/$(id -u)/$AGENT_LABEL" >/dev/null 2>&1
  launchctl bootstrap "gui/$(id -u)" "$AGENT_PLIST" >>"$LOG" 2>&1 ||
    log "the reconcile agent starts at the next login"
}

apply() {
  [[ -f $PINNED ]] || fail "pinned.json is missing"
  : >"$LOG"
  mkdir -p "$WORK" || fail "could not prepare the work folder"
  sip_disabled || fail "System Integrity Protection is on, so the privacy grants cannot be written"
  install_app
  install_lcu
  say "Granting Accessibility and Screen Recording"
  grant_clients
  write_ledger || fail "could not write the Screen Recording approvals"
  restart_tccd
  register
  logged "$LCU" doctor || fail "lcu doctor reported a problem"
  rm -rf "$WORK"
  write_receipt ready ""
  say "Computer use is ready"
}

command=${1:-}
[[ $# -gt 0 ]] && shift
case $command in
  apply)
    [[ ${1:-} == --approval && ( ${2:-} == ask || ${2:-} == auto ) ]] || fail "usage: apply --approval ask|auto"
    APPROVAL=$2
    apply
    ;;
  app-present)
    [[ -f $PINNED ]] && app_ok "$APP"
    ;;
  status)
    cat "$RECEIPT" 2>/dev/null || print '{"state":"none"}'
    ;;
  safe-members)
    safe_members
    ;;
  links-inside)
    links_stay_inside "$1"
    ;;
  grant)
    mkdir -p "$WORK"
    [[ $# -eq 3 ]] || fail "usage: grant SERVICE CLIENT PATH"
    grant "$1" "$2" "$3"
    ;;
  *)
    print -u2 "usage: silo-computer-use.zsh apply --approval ask|auto | app-present | status"
    exit 2
    ;;
esac
