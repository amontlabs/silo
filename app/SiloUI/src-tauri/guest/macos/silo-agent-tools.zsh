#!/bin/zsh
# Installs one agent (Claude Code or Codex) in a Silo macOS computer from files the host
# downloaded, checked against Silo's lock and staged here. The computer downloads nothing
# itself, apart from Anthropic's own installer fetching Claude Code. Silo never signs in to an agent.
set -eu
setopt pipefail extendedglob

STAGE=/private/tmp/silo-agent-tools
LOG=$HOME/Library/Logs/silo-agent-tools.log
AGENT='@AGENT@'
VERSION='@VERSION@'
FILE='@FILE@'
BYTES='@BYTES@'
SHA256='@SHA256@'
MEMBER='@MEMBER@'
LOCK_SHA256='@LOCK_SHA256@'
TOOLS=/usr/local/libexec/silo-tools
RECEIPT_DIR='/Library/Application Support/Silo/agents'
SAFE=

export PATH="/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
mkdir -p "$HOME/Library/Logs"
: >"$LOG"
log() { print -r -- "$(date '+%Y-%m-%d %H:%M:%S') $*" >>"$LOG"; }
say() { log "$*"; print -r -- "$*"; }
cleanup() {
  /bin/rm -rf "$STAGE"
  [[ -z $SAFE ]] || /usr/bin/sudo -n /bin/rm -rf "$SAFE"
}
trap cleanup EXIT
fail() { log "error: $*"; print -u2 -r -- "error: $*"; exit 1; }
logged() {
  log "\$ $*"
  "$@" >>"$LOG" 2>&1 || fail "command failed: $*"
}
safe_member() {
  [[ -n $1 && $1 != /* && $1 != .. && $1 != ../* && $1 != */../* && $1 != */.. ]]
}
# The version in a program's output: its first word of dotted numbers.
version_token() {
  local word
  for word in ${=1}; do
    if [[ $word == [0-9]##(.[0-9]##)# ]]; then
      print -r -- $word
      return 0
    fi
  done
  return 1
}
# Succeeds when version $1 is the same as or newer than version $2.
not_older() {
  local -a have want
  local i count
  have=(${(s:.:)1})
  want=(${(s:.:)2})
  count=${#have}
  (( ${#want} > count )) && count=${#want}
  for (( i = 1; i <= count; i++ )); do
    (( 10#${have[i]:-0} > 10#${want[i]:-0} )) && return 0
    (( 10#${have[i]:-0} < 10#${want[i]:-0} )) && return 1
  done
  return 0
}

[[ $(/usr/bin/uname -m) == arm64 ]] || fail "the locked agents require Apple silicon"
[[ -f $STAGE/$FILE && ! -L $STAGE/$FILE ]] || fail "missing staged file: $FILE"

# The staged folder belongs to the account, which could replace the file after it was checked.
# The checked copy lives in a folder only root can change, and everything below uses that copy.
logged /usr/bin/sudo -n /usr/bin/install -d -o root -g wheel -m 0755 "$TOOLS"
SAFE=$(/usr/bin/sudo -n /usr/bin/mktemp -d "$TOOLS/stage.XXXXXXXX") || fail "could not prepare a folder for the install"
logged /usr/bin/sudo -n /bin/chmod 0755 "$SAFE"
logged /usr/bin/sudo -n /usr/bin/install -o root -g wheel -m 0644 "$STAGE/$FILE" "$SAFE/$FILE"
WORK=$SAFE/work
logged /usr/bin/sudo -n /usr/bin/install -d -o root -g wheel -m 0755 "$WORK"
size=$(/usr/bin/stat -f '%z' "$SAFE/$FILE")
digest=$(/usr/bin/shasum -a 256 "$SAFE/$FILE" | /usr/bin/awk '{print $1}')
[[ $size == $BYTES && $digest == $SHA256 ]] || fail "staged file did not match its locked size and SHA-256: $FILE"

# What the receipt records: the version the computer ends up with.
RECEIPT_VERSION=$VERSION
RECEIPT_SHA256=$SHA256

install_codex() {
  local current
  current=$(version_token "$(/usr/bin/env -i "HOME=$HOME" "PATH=/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin" /usr/local/bin/codex --version 2>/dev/null || true)" || true)
  if [[ -n $current && $current != $VERSION ]] && not_older "$current" "$VERSION"; then
    say "Codex $current is newer than $VERSION and stays installed"
    RECEIPT_VERSION=$current
    RECEIPT_SHA256=
    return 0
  fi
  members=$(/usr/bin/tar -tzf "$SAFE/$FILE") || fail "could not inspect the Codex archive"
  [[ $members == $MEMBER ]] && safe_member "$members" || fail "the Codex archive does not hold the expected file"
  logged /usr/bin/sudo -n /usr/bin/tar -xzf "$SAFE/$FILE" -C "$WORK" --no-same-owner
  [[ -f $WORK/$MEMBER && ! -L $WORK/$MEMBER ]] || fail "the unpacked Codex file is missing"
  logged /usr/bin/sudo -n /bin/chmod 0755 "$WORK/$MEMBER"
  unpacked=$("$WORK/$MEMBER" --version 2>>"$LOG") || fail "Codex did not run"
  [[ $(version_token "$unpacked") == $VERSION ]] || fail "Codex version verification failed"
  target=$TOOLS/codex/$VERSION
  logged /usr/bin/sudo -n /usr/bin/install -d -o root -g wheel -m 0755 "$target" /usr/local/bin
  logged /usr/bin/sudo -n /usr/bin/install -o root -g wheel -m 0755 "$WORK/$MEMBER" "$target/codex"
  logged /usr/bin/sudo -n /bin/ln -sfn "$target/codex" /usr/local/bin/codex
  for older in $TOOLS/codex/*(N/); do
    [[ $older == $target ]] || logged /usr/bin/sudo -n /bin/rm -rf "$older"
  done
  installed=$(/usr/bin/env -i "HOME=$HOME" "PATH=/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin" /usr/local/bin/codex --version) || fail "Codex did not run"
  [[ $(version_token "$installed") == $VERSION ]] || fail "Codex version verification failed"
}

install_claude_code() {
  local current
  current=$(version_token "$(/usr/bin/env "HOME=$HOME" /usr/local/bin/claude --version 2>/dev/null || true)" || true)
  if [[ -n $current ]] && not_older "$current" "$VERSION"; then
    if [[ $current == $VERSION ]]; then
      say "Claude Code $VERSION is already installed"
    else
      say "Claude Code $current is newer than $VERSION and stays installed"
      RECEIPT_VERSION=$current
      RECEIPT_SHA256=
    fi
    return 0
  fi
  # Anthropic's documented installer (the staged file is the one the lock pins). It checks
  # the binary against Anthropic's release manifest. Auto-update stays off so the pinned
  # version holds.
  logged /usr/bin/env "HOME=$HOME" "DISABLE_AUTOUPDATER=1" "PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
    /bin/bash "$SAFE/$FILE" "$VERSION"
  [[ -x $HOME/.local/bin/claude ]] || fail "the Claude Code installer did not create ~/.local/bin/claude"
  logged /usr/bin/sudo -n /usr/bin/install -d -o root -g wheel -m 0755 /usr/local/bin
  logged /usr/bin/sudo -n /bin/ln -sfn "$HOME/.local/bin/claude" /usr/local/bin/claude
  installed=$(/usr/bin/env "HOME=$HOME" /usr/local/bin/claude --version) || fail "Claude Code did not run"
  [[ $(version_token "$installed") == $VERSION ]] || fail "Claude Code version verification failed"
}

case $AGENT in
  codex) install_codex ;;
  claude-code) install_claude_code ;;
  *) fail "unknown agent: $AGENT" ;;
esac

printf '{"schemaVersion":1,"agent":"%s","version":"%s","sha256":"%s","lockDigest":"%s","installedAt":"%s"}\n' \
  "$AGENT" "$RECEIPT_VERSION" "$RECEIPT_SHA256" "$LOCK_SHA256" "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
  | /usr/bin/sudo -n /usr/bin/tee "$SAFE/$AGENT.json" >/dev/null
logged /usr/bin/sudo -n /usr/bin/install -d -o root -g wheel -m 0755 "$RECEIPT_DIR"
logged /usr/bin/sudo -n /usr/bin/install -o root -g wheel -m 0644 "$SAFE/$AGENT.json" "$RECEIPT_DIR/$AGENT.json"
say "$AGENT $RECEIPT_VERSION is ready"
