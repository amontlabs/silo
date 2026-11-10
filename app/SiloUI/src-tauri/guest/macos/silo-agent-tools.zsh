#!/bin/zsh
# Installs one agent (Claude Code or Codex) in a Silo macOS computer from files the host
# downloaded, checked against Silo's lock and staged here. The computer downloads nothing
# itself, apart from Anthropic's own installer fetching Claude Code. Silo never signs in to an agent.
set -eu
setopt pipefail

STAGE=/private/tmp/silo-agent-tools
WORK=$STAGE/work
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

export PATH="/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
mkdir -p "$HOME/Library/Logs" "$WORK"
: >"$LOG"
log() { print -r -- "$(date '+%Y-%m-%d %H:%M:%S') $*" >>"$LOG"; }
say() { log "$*"; print -r -- "$*"; }
fail() { log "error: $*"; print -u2 -r -- "error: $*"; exit 1; }
logged() {
  log "\$ $*"
  "$@" >>"$LOG" 2>&1 || fail "command failed: $*"
}
safe_member() {
  [[ -n $1 && $1 != /* && $1 != .. && $1 != ../* && $1 != */../* && $1 != */.. ]]
}

[[ $(/usr/bin/uname -m) == arm64 ]] || fail "the locked agents require Apple silicon"
[[ -f $STAGE/$FILE ]] || fail "missing staged file: $FILE"
size=$(/usr/bin/stat -f '%z' "$STAGE/$FILE")
digest=$(/usr/bin/shasum -a 256 "$STAGE/$FILE" | /usr/bin/awk '{print $1}')
[[ $size == $BYTES && $digest == $SHA256 ]] || fail "staged file did not match its locked size and SHA-256: $FILE"

install_codex() {
  members=$(/usr/bin/tar -tzf "$STAGE/$FILE") || fail "could not inspect the Codex archive"
  [[ $members == $MEMBER ]] && safe_member "$members" || fail "the Codex archive does not hold the expected file"
  /usr/bin/tar -xzf "$STAGE/$FILE" -C "$WORK" || fail "could not unpack Codex"
  [[ -f $WORK/$MEMBER && ! -L $WORK/$MEMBER ]] || fail "the unpacked Codex file is missing"
  /bin/chmod 0755 "$WORK/$MEMBER"
  unpacked=$("$WORK/$MEMBER" --version 2>>"$LOG") || fail "Codex did not run"
  [[ $unpacked == *"$VERSION"* ]] || fail "Codex version verification failed"
  target=$TOOLS/codex/$VERSION
  logged /usr/bin/sudo -n /usr/bin/install -d -o root -g wheel -m 0755 "$target" /usr/local/bin
  logged /usr/bin/sudo -n /usr/bin/install -o root -g wheel -m 0755 "$WORK/$MEMBER" "$target/codex"
  logged /usr/bin/sudo -n /bin/ln -sfn "$target/codex" /usr/local/bin/codex
  for older in $TOOLS/codex/*(N/); do
    [[ $older == $target ]] || logged /usr/bin/sudo -n /bin/rm -rf "$older"
  done
  installed=$(/usr/bin/env -i "HOME=$HOME" "PATH=/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin" /usr/local/bin/codex --version) || fail "Codex did not run"
  [[ $installed == *"$VERSION"* ]] || fail "Codex version verification failed"
}

install_claude_code() {
  current=$(/usr/bin/env "HOME=$HOME" /usr/local/bin/claude --version 2>/dev/null || true)
  if [[ $current == "$VERSION"* ]]; then
    say "Claude Code $VERSION is already installed"
  else
    # Anthropic's documented installer (the staged file is the one the lock pins). It checks
    # the binary against Anthropic's release manifest. Auto-update stays off so the pinned
    # version holds.
    logged /usr/bin/env "HOME=$HOME" "DISABLE_AUTOUPDATER=1" "PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
      /bin/bash "$STAGE/$FILE" "$VERSION"
    [[ -x $HOME/.local/bin/claude ]] || fail "the Claude Code installer did not create ~/.local/bin/claude"
    logged /usr/bin/sudo -n /usr/bin/install -d -o root -g wheel -m 0755 /usr/local/bin
    logged /usr/bin/sudo -n /bin/ln -sfn "$HOME/.local/bin/claude" /usr/local/bin/claude
  fi
  installed=$(/usr/bin/env "HOME=$HOME" /usr/local/bin/claude --version) || fail "Claude Code did not run"
  [[ $installed == "$VERSION"* ]] || fail "Claude Code version verification failed"
}

case $AGENT in
  codex) install_codex ;;
  claude-code) install_claude_code ;;
  *) fail "unknown agent: $AGENT" ;;
esac

receipt_tmp="$WORK/$AGENT.json"
printf '{"schemaVersion":1,"agent":"%s","version":"%s","sha256":"%s","lockDigest":"%s","installedAt":"%s"}\n' \
  "$AGENT" "$VERSION" "$SHA256" "$LOCK_SHA256" "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" >"$receipt_tmp"
logged /usr/bin/sudo -n /usr/bin/install -d -o root -g wheel -m 0755 "$RECEIPT_DIR"
logged /usr/bin/sudo -n /usr/bin/install -o root -g wheel -m 0644 "$receipt_tmp" "$RECEIPT_DIR/$AGENT.json"
say "$AGENT $VERSION is ready"
/bin/rm -rf "$STAGE"
