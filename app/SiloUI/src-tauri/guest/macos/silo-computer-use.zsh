#!/bin/zsh
# Computer use for a Silo macOS guest: Cua Driver (trycua/cua, MIT) and the TCC grants that
# stand in for the "Allow" dialogs.
#
# Silo copies this script, pinned.json, silo-cua-register.py and the driver archive named in
# pinned.json into one folder in the guest and runs it as the working account (`silo`, with
# passwordless sudo and System Integrity Protection off):
#
#   silo-computer-use.zsh apply --approval ask|auto   install, grant, register; idempotent
#   silo-computer-use.zsh app-present                 exit 0 when the pinned app is installed
#   silo-computer-use.zsh status                      print the receipt
#   silo-computer-use.zsh grant SERVICE CLIENT PATH   write one TCC row (used by tests)
#
# `apply` steps:
#   1. check that System Integrity Protection is off, and remove what the earlier ChatGPT app
#      and LCU setup left (the app, LCU, their TCC rows, their LaunchAgent and registrations);
#   2. switch Cua Driver's telemetry and update check off (environment for this run, Cua Driver's
#      own config for the daemon that LaunchServices starts, the user's zsh environment);
#   3. install /Applications/CuaDriver.app from the staged archive when the pinned version is not
#      there: SHA-256, code signature (`codesign --verify --deep --strict`), bundle identifier
#      and team are checked, then the app is moved in; link `cua-driver` onto PATH
#      (/usr/local/bin/cua-driver -> the app's executable, which Cua Driver needs to start its
#      daemon under the app's own identity);
#   4. grant Accessibility and Screen Recording to the app (`com.trycua.driver`) in the system
#      TCC database, install the guest ScreenCaptureKit recorder as its own Aqua LaunchAgent, and
#      pre-write the Screen Recording consent ledger;
#   5. turn on Screen Sharing (the guest's built-in VNC server, which Silo reaches through a
#      bridge from the controlling device) and grant its two clients Screen Recording,
#      Accessibility and Post Event, as the "Allow" dialogs would;
#   6. register the `cua-driver` MCP server with Claude Code and Codex (silo-cua-register.py),
#      with the approval switch applied, and install a LaunchAgent that registers agents
#      installed later at each login;
#   7. check readiness: `cua-driver permissions status --json` from the daemon (started through
#      LaunchServices when the computer has a login session) and `cua-driver doctor --json`.
# Cua Driver's own per-app consent stays with Cua Driver's permission mode and is never seeded.
#
# No LaunchAgent starts the daemon: `cua-driver mcp` starts `CuaDriver.app` with `open` when an
# agent opens its first session, which keeps the daemon under the app's TCC identity.
#
# The TCC row layout and csreq derivation follow trycua/cua `seed-tcc.sh` and actions/runner-images
# `configure-tccdb-macos.sh` (both MIT); the Screen Recording ledger follows electron/electron's
# `screencapture-nag-remover.sh` (MIT) and Cua Driver's own consent check, which counts an entry
# keyed by the bundle identifier.
#
# Output: progress lines on stdout, `error: <reason>` on stderr and exit 1 on failure. The full
# log is ~/Library/Logs/silo-computer-use.log; the receipt is
# ~/Library/Application Support/Silo/computer-use-receipt.json.

set -u
set -o pipefail
export PATH=/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin
# Cua Driver sends telemetry and checks for updates by default; every command of this script
# runs with both off.
export CUA_DRIVER_RS_TELEMETRY_ENABLED=0 CUA_DRIVER_RS_UPDATE_CHECK=false DO_NOT_TRACK=1

STAGE=${0:A:h}
PINNED=$STAGE/pinned.json
LOG=$HOME/Library/Logs/silo-computer-use.log
RECEIPT_DIR=$HOME/Library/Application\ Support/Silo
RECEIPT=$RECEIPT_DIR/computer-use-receipt.json
WORK=${SILO_CU_WORK:-$STAGE/work}
APP=/Applications/CuaDriver.app
DRIVER=/usr/local/bin/cua-driver
APP_EXECUTABLE=$APP/Contents/MacOS/cua-driver
REGISTER_SOURCE=$STAGE/silo-cua-register.py
REGISTER=$RECEIPT_DIR/silo-cua-register.py
PYTHON=/usr/bin/python3
ZSHENV=$HOME/.zshenv
LEGACY_APP=/Applications/ChatGPT.app
LEGACY_HELPER_REL="Contents/Resources/cua_node/lib/node_modules/@oai/sky/Codex Computer Use.app"
TCC_DB=${SILO_CU_TCC_DB:-/Library/Application Support/com.apple.TCC/TCC.db}
SUDO=${SILO_CU_SUDO-sudo -n}
AGENT_LABEL=org.silo.computer-use-reconcile
AGENT_PLIST=$HOME/Library/LaunchAgents/$AGENT_LABEL.plist
RECORDER_ID=org.silo.guest-recorder
RECORDER_APP=$HOME/Applications/SiloGuestRecorder.app
RECORDER_BINARY=$RECORDER_APP/Contents/MacOS/SiloGuestRecorder
RECORDER_AGENT_LABEL=org.silo.guest-recorder
RECORDER_AGENT_PLIST=$HOME/Library/LaunchAgents/$RECORDER_AGENT_LABEL.plist
RECORDER_OUTPUT=$HOME/Library/Application\ Support/Silo/Recordings/guest-screen.mov
RECORDER_SOURCE=$STAGE/SiloGuestRecorder.swift
RECORDER_INFO=$STAGE/SiloGuestRecorder-Info.plist
RECORDER_AGENT=$STAGE/SiloGuestRecorder-LaunchAgent.plist
LEDGER=$HOME/Library/Group\ Containers/group.com.apple.replayd/ScreenCaptureApprovals.plist
SERVICES=(kTCCServiceAccessibility kTCCServiceScreenCapture)
SHARING_SERVICES=(kTCCServiceScreenCapture kTCCServiceAccessibility kTCCServicePostEvent)
SHARING_CLIENTS=(
  "com.apple.screensharing.agent /System/Library/CoreServices/RemoteManagement/ScreensharingAgent.bundle"
  "com.apple.screensharing.daemon /System/Library/CoreServices/RemoteManagement/screensharingd.bundle"
)
SHARING_JOB=system/com.apple.screensharing
SHARING_PLIST=/System/Library/LaunchDaemons/com.apple.screensharing.plist
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

# The agents the registration helper recorded as JSON lists, `[]` when it has not run.
agent_list() { # key
  local list
  list=$(plutil -extract "$1" json -o - "$RECEIPT_DIR/computer-use-agents.json" 2>/dev/null)
  [[ $list == \[*\] && $list != *[^A-Za-z0-9\",\[\]-]* ]] && print -r -- "$list" || print -r -- '[]'
}

write_receipt() { # state reason
  printf '{"schemaVersion":2,"state":"%s","reason":"%s","driverVersion":"%s","approval":"%s","agents":%s,"pending":%s,"at":"%s"}\n' \
    "$1" "${2//[^A-Za-z0-9 .,:_-]/}" "$(pinned driverVersion)" "$APPROVAL" "$(agent_list agents)" "$(agent_list pending)" \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >"$RECEIPT.tmp" &&
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
  # Judged on the link text alone, without following links: a link may point at a file that only
  # exists after installation, but never outside the tree.
  local root=${1:A} link text candidate target
  find "$root" -type l -print0 | while IFS= read -r -d '' link; do
    text=$(readlink "$link") || return 1
    [[ -n $text && $text != /* ]] || return 1
    candidate=${link:h}/$text
    target=${candidate:a}
    [[ $target == $root || $target == $root/* ]] || return 1
  done
}

# MARK: Cua Driver

app_info() { /usr/libexec/PlistBuddy -c "Print :$2" "$1/Contents/Info.plist" 2>/dev/null; }

# Whether the bundle at $1 is the pinned, officially signed app.
app_ok() {
  local code=$1 team
  [[ -d $code ]] || return 1
  [[ $(app_info "$code" CFBundleIdentifier) == "$(pinned bundleIdentifier)" ]] || return 1
  [[ $(app_info "$code" CFBundleShortVersionString) == "$(pinned driverVersion)" ]] || return 1
  team=$(codesign -dv "$code" 2>&1 | sed -n 's/^TeamIdentifier=//p')
  [[ $team == "$(pinned teamIdentifier)" ]] || return 1
  codesign --verify --deep --strict "$code" >>"$LOG" 2>&1
}

stop_daemon() { /usr/bin/pkill -f "$APP_EXECUTABLE" >>"$LOG" 2>&1; sleep 1; return 0; }

install_app() {
  if app_ok "$APP"; then
    say "Cua Driver $(pinned driverVersion) is already installed"
    return 0
  fi
  say "Installing Cua Driver $(pinned driverVersion)"
  local archive=$STAGE/$(pinned driverArchive)
  check_sha "$archive" "$(pinned driverSha256)" "The Cua Driver archive"
  rm -rf "$WORK/cua" && mkdir -p "$WORK/cua" || fail "could not prepare the Cua Driver folder"
  tar -tzf "$archive" 2>>"$LOG" | safe_members || fail "the Cua Driver archive has unsafe paths"
  logged tar -xzf "$archive" -C "$WORK/cua" || fail "could not unpack Cua Driver"
  links_stay_inside "$WORK/cua" || fail "the Cua Driver archive has links that leave its folder"
  local fresh=$WORK/cua/${$(pinned driverArchive)%.tar.gz}/CuaDriver.app
  app_ok "$fresh" || fail "Cua Driver is not the official signed app"
  # A running daemon holds the old executable; the replacement starts it again.
  stop_daemon
  if [[ -e $APP ]]; then
    logged ${=SUDO} rm -rf "$APP" || fail "could not remove the previous Cua Driver"
  fi
  logged ${=SUDO} mv "$fresh" "$APP" || fail "could not move Cua Driver into Applications"
  logged ${=SUDO} chown -R root:wheel "$APP" || fail "could not hand Cua Driver to root"
  ${=SUDO} xattr -dr com.apple.quarantine "$APP" >>"$LOG" 2>&1
  app_ok "$APP" || fail "the installed Cua Driver did not verify"
  # Launch Services must resolve the bundle identifier to this copy when the daemon starts.
  local lsregister=/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister
  [[ -x $lsregister ]] && logged "$lsregister" -f "$APP"
  rm -rf "$WORK/cua" "$archive"
}

# Links `cua-driver` onto PATH through the app's executable: Cua Driver starts its daemon under
# the app's own identity only when it runs from inside CuaDriver.app.
link_driver() {
  logged ${=SUDO} /usr/bin/install -d -o root -g wheel -m 0755 /usr/local/bin || fail "could not prepare /usr/local/bin"
  logged ${=SUDO} /bin/ln -sfn "$APP_EXECUTABLE" "$DRIVER" || fail "could not link cua-driver onto PATH"
  [[ $("$DRIVER" --version 2>>"$LOG") == "cua-driver $(pinned driverVersion)" ]] ||
    fail "cua-driver did not report the pinned version"
}

# Switches telemetry and the update check off for the daemon, which LaunchServices starts
# without this environment, and for the user's shells.
switch_privacy_off() {
  [[ -f $REGISTER_SOURCE ]] || fail "the registration helper was not copied to the computer"
  "$PYTHON" -c 'import sys' >>"$LOG" 2>&1 || fail "Python 3 from the Command Line Tools is unavailable"
  mkdir -p "$RECEIPT_DIR" && cp "$REGISTER_SOURCE" "$REGISTER" || fail "could not install the registration helper"
  logged "$PYTHON" "$REGISTER" privacy || fail "could not switch Cua Driver telemetry off"
  local line
  for line in 'export CUA_DRIVER_RS_TELEMETRY_ENABLED=0' 'export CUA_DRIVER_RS_UPDATE_CHECK=false'; do
    /usr/bin/grep -Fqx "$line" "$ZSHENV" 2>/dev/null || print -r -- "$line" >>"$ZSHENV" || fail "could not update $ZSHENV"
  done
  launchctl setenv CUA_DRIVER_RS_TELEMETRY_ENABLED 0 >>"$LOG" 2>&1
  launchctl setenv CUA_DRIVER_RS_UPDATE_CHECK false >>"$LOG" 2>&1
  return 0
}

# MARK: Earlier ChatGPT app and LCU

# Removes the ChatGPT app, LCU, their TCC rows and ledger entries, registrations and the old
# reconcile LaunchAgent from a computer set up before Cua Driver.
remove_legacy() {
  local helper=$LEGACY_APP/$LEGACY_HELPER_REL code client
  local clients=()
  for code in "$LEGACY_APP" "$helper"; do
    client=$(app_info "$code" CFBundleIdentifier)
    [[ -n $client ]] && clients+=("$client")
  done
  clients+=(com.openai.codex)
  launchctl bootout "gui/$(id -u)/$AGENT_LABEL" >/dev/null 2>&1
  if [[ -d $LEGACY_APP || -e $HOME/.local/share/lcu || -e $HOME/.local/state/lcu ]]; then
    say "Removing the earlier ChatGPT app and LCU"
  fi
  for client in ${(u)clients}; do
    ${=SUDO} sqlite3 "$TCC_DB" "delete from access where client='$client';" >>"$LOG" 2>&1
    defaults delete "$LEDGER" "$client" >/dev/null 2>&1
  done
  # Registrations (Claude Code, Codex) are removed by silo-cua-register.py; the files LCU wrote
  # for its other agents go with its folders.
  rm -rf "$HOME/.local/share/lcu" "$HOME/.local/state/lcu" "$HOME"/.local/bin/lcu*(N) "$HOME/.hermes/plugins/lcu-cua" ||
    fail "could not remove LCU"
  if [[ -e $LEGACY_APP ]]; then
    logged ${=SUDO} rm -rf "$LEGACY_APP" || fail "could not remove the ChatGPT app"
  fi
  return 0
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
  local client service
  client=$(app_info "$APP" CFBundleIdentifier)
  [[ $client == "$(pinned bundleIdentifier)" ]] || fail "could not read the identifier of ${APP:t}"
  for service in $SERVICES; do
    grant "$service" "$client" "$APP" || fail "could not write the $service grant for $client"
  done
  for service in $SERVICES; do
    granted "$service" "$client" || fail "the $service grant for $client did not read back"
  done
}

install_recorder() {
  [[ -f $RECORDER_SOURCE && -f $RECORDER_INFO ]] || fail "the guest recorder source was not copied to the computer"
  say "Building the guest ScreenCaptureKit recorder"
  local build=$WORK/SiloGuestRecorder.app
  rm -rf "$build" "$RECORDER_APP"
  mkdir -p "$build/Contents/MacOS" "${RECORDER_APP:h}" || fail "could not prepare the guest recorder bundle"
  logged /usr/bin/swiftc -parse-as-library -O \
    -framework AVFoundation -framework CoreMedia -framework CoreVideo -framework Foundation -framework ScreenCaptureKit \
    "$RECORDER_SOURCE" -o "$build/Contents/MacOS/SiloGuestRecorder" || fail "could not build the guest ScreenCaptureKit recorder"
  cp "$RECORDER_INFO" "$build/Contents/Info.plist" || fail "could not prepare the guest recorder metadata"
  logged /usr/bin/codesign --force --sign - --identifier "$RECORDER_ID" --timestamp=none "$build" ||
    fail "could not sign the guest recorder bundle"
  logged /usr/bin/codesign --verify --deep --strict "$build" || fail "the guest recorder signature did not verify"
  /usr/bin/ditto "$build" "$RECORDER_APP" || fail "could not install the guest recorder bundle"
  [[ $(app_info "$RECORDER_APP" CFBundleIdentifier) == "$RECORDER_ID" ]] || fail "the guest recorder bundle identifier is wrong"
}

grant_recorder() {
  local client
  client=$(app_info "$RECORDER_APP" CFBundleIdentifier)
  [[ $client == "$RECORDER_ID" ]] || fail "the guest recorder is not $RECORDER_ID"
  grant kTCCServiceScreenCapture "$client" "$RECORDER_APP" || fail "could not write the Screen Recording grant for the guest recorder"
  granted kTCCServiceScreenCapture "$client" || fail "the guest recorder's Screen Recording grant did not read back"
}

# Screen Sharing's agent and daemon need the rows an MDM's remote desktop command writes, or a
# viewer gets a black screen and no input. Type 2 (VNC password) authentication is not switched
# on: viewers sign in with the computer's account.
grant_sharing() {
  local entry client code service
  for entry in $SHARING_CLIENTS; do
    client=${entry%% *} code=${entry#* }
    [[ -d $code ]] || fail "this macOS has no ${code:t}, so Screen Sharing is unavailable"
    [[ $(app_info "$code" CFBundleIdentifier) == "$client" ]] || fail "${code:t} is not $client"
    for service in $SHARING_SERVICES; do
      grant "$service" "$client" "$code" || fail "could not write the $service grant for $client"
    done
  done
  for entry in $SHARING_CLIENTS; do
    client=${entry%% *}
    for service in $SHARING_SERVICES; do
      granted "$service" "$client" || fail "the $service grant for $client did not read back"
    done
  done
}

# Enables and loads Screen Sharing; running it again changes nothing.
enable_sharing() {
  local state greeting
  [[ -f $SHARING_PLIST ]] || fail "this macOS has no Screen Sharing service"
  logged ${=SUDO} launchctl enable $SHARING_JOB || fail "could not enable Screen Sharing"
  if ! ${=SUDO} launchctl print $SHARING_JOB >/dev/null 2>&1; then
    logged ${=SUDO} launchctl bootstrap system "$SHARING_PLIST" || fail "could not load Screen Sharing"
  fi
  for _ in {1..30}; do
    # launchd socket-activates this job. Its loaded state can be `not running`
    # between connections; opening TCP starts screensharingd and returns its RFB greeting.
    if state=$(${=SUDO} launchctl print $SHARING_JOB 2>/dev/null | sed -n 's/^[[:space:]]*state = //p' | head -n 1); then
      greeting=$(/usr/bin/nc -w 1 127.0.0.1 5900 </dev/null 2>/dev/null | /usr/bin/head -c 11 || true)
      if [[ $greeting =~ '^RFB 003\.[0-9]{3}$' ]]; then
        say "Screen Sharing is loaded and its RFB server greeted on TCP port 5900 (launchd state: $state)"
        return 0
      fi
    fi
    sleep 1
  done
  fail "Screen Sharing did not return an RFB greeting on TCP port 5900"
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
  for code in "$APP" "$RECORDER_APP"; do
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

install_recorder_agent() {
  [[ -f $RECORDER_AGENT ]] || fail "the guest recorder LaunchAgent was not copied to the computer"
  mkdir -p "${RECORDER_AGENT_PLIST:h}" || fail "could not prepare the guest recorder LaunchAgents folder"
  sed \
    -e "s|@@RECORDER_BINARY@@|$RECORDER_BINARY|g" \
    -e "s|@@RECORDER_OUTPUT@@|$RECORDER_OUTPUT|g" \
    -e "s|@@HOME@@|$HOME|g" \
    "$RECORDER_AGENT" >"$RECORDER_AGENT_PLIST" || fail "could not write the guest recorder LaunchAgent"
  plutil -lint "$RECORDER_AGENT_PLIST" >>"$LOG" 2>&1 || fail "the guest recorder LaunchAgent is not a valid property list"
  # SSH can finish setup before the console account has an Aqua launchd domain.
  # Keeping the plist in LaunchAgents lets launchd load it at that account's next login.
  if ! launchctl print "gui/$(id -u)" >/dev/null 2>&1; then
    log "the guest recorder loads at the next Aqua login"
    return 0
  fi
  launchctl bootout "gui/$(id -u)/$RECORDER_AGENT_LABEL" >/dev/null 2>&1
  launchctl bootstrap "gui/$(id -u)" "$RECORDER_AGENT_PLIST" >>"$LOG" 2>&1 || fail "could not load the guest recorder Aqua LaunchAgent"
  launchctl print "gui/$(id -u)/$RECORDER_AGENT_LABEL" >/dev/null 2>&1 || fail "the guest recorder Aqua LaunchAgent did not load"
}

app_executable() { print -r -- "$1/Contents/MacOS/$(app_info "$1" CFBundleExecutable)"; }

restart_tccd() {
  launchctl kickstart -k "gui/$(id -u)/com.apple.tccd" >>"$LOG" 2>&1
  ${=SUDO} launchctl kickstart -k system/com.apple.tccd.system >>"$LOG" 2>&1 ||
    log "the system tccd was not restarted; the grants apply after the next boot"
}

# MARK: Registration

register() {
  say "Registering Cua Driver with every agent"
  logged "$PYTHON" "$REGISTER" apply --approval "$APPROVAL" || fail "could not register Cua Driver with the agents"
  install_agent
}

install_agent() {
  mkdir -p "${AGENT_PLIST:h}" || fail "could not create the LaunchAgents folder"
  cat >"$AGENT_PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>$AGENT_LABEL</string>
<key>ProgramArguments</key><array><string>$PYTHON</string><string>$REGISTER</string><string>reconcile</string></array>
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

# MARK: Readiness

json_value() { # key; the JSON on stdin
  plutil -extract "$1" raw -o - - 2>/dev/null
}

# The daemon answers `permissions status` for com.trycua.driver; without one the answer is
# `unknown`, so a login session is needed to start it (`cua-driver mcp` does the same).
check_permissions_status() {
  if ! launchctl print "gui/$(id -u)" >/dev/null 2>&1; then
    log "no login session yet: the daemon starts with the first agent session"
    return 0
  fi
  stop_daemon
  say "Starting the Cua Driver daemon"
  logged /usr/bin/open -n -g "$APP" --args serve || fail "could not start the Cua Driver daemon"
  local out ax sr
  for _ in {1..60}; do
    out=$("$DRIVER" permissions status --json 2>>"$LOG")
    ax=$(print -r -- "$out" | json_value accessibility)
    sr=$(print -r -- "$out" | json_value screen_recording)
    if [[ $ax == true && $sr == true ]]; then
      log "cua-driver permissions status: $out"
      return 0
    fi
    sleep 1
  done
  log "cua-driver permissions status: $out"
  fail "the Cua Driver daemon does not report Accessibility and Screen Recording as granted"
}

check_doctor() {
  local out
  out=$("$DRIVER" doctor --json 2>>"$LOG")
  log "cua-driver doctor: $out"
  [[ $(print -r -- "$out" | json_value ok) == true ]] || fail "cua-driver doctor reported a problem"
}

apply() {
  [[ -f $PINNED ]] || fail "pinned.json is missing"
  : >"$LOG"
  mkdir -p "$WORK" || fail "could not prepare the work folder"
  sip_disabled || fail "System Integrity Protection is on, so the privacy grants cannot be written"
  switch_privacy_off
  remove_legacy
  install_app
  link_driver
  install_recorder
  say "Granting Accessibility and Screen Recording"
  grant_clients
  say "Granting Screen Recording to the guest recorder"
  grant_recorder
  write_ledger || fail "could not write the Screen Recording approvals"
  say "Turning on Screen Sharing"
  grant_sharing
  restart_tccd
  enable_sharing
  install_recorder_agent
  register
  check_permissions_status
  check_doctor
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
  sharing-grants)
    for entry in $SHARING_CLIENTS; do
      for service in $SHARING_SERVICES; do print -r -- "$service ${entry%% *}"; done
    done
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
