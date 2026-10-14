#!/bin/zsh
set -eu
setopt pipefail

STAGE=/private/tmp/silo-developer-tools
WORK=$STAGE/work
LOG=$HOME/Library/Logs/silo-developer-tools.log
CLT_DIRECTORY=/Library/Developer/CommandLineTools
TOOLS_VERSION='@TOOLS_VERSION@'
SUPPORTED_GUEST_BUILDS=( @SUPPORTED_GUEST_BUILDS@ )
APPLE_SIGNER='@APPLE_SIGNER@'
APPLE_SIGNER_SHA256='@APPLE_SIGNER_SHA256@'
CLT_VERSION='@CLT_VERSION@'
CLT_PACKAGES=(
@CLT_PACKAGES@
)
GIT_LFS_VERSION='@GIT_LFS_VERSION@'
GIT_LFS_FILE='@GIT_LFS_FILE@'
GIT_LFS_BYTES='@GIT_LFS_BYTES@'
GIT_LFS_SHA256='@GIT_LFS_SHA256@'
GH_VERSION='@GH_VERSION@'
GH_FILE='@GH_FILE@'
GH_BYTES='@GH_BYTES@'
GH_SHA256='@GH_SHA256@'
NODE_VERSION='@NODE_VERSION@'
NODE_FILE='@NODE_FILE@'
NODE_BYTES='@NODE_BYTES@'
NODE_SHA256='@NODE_SHA256@'
CLAUDE_VERSION='@CLAUDE_VERSION@'
CLAUDE_INSTALLER_FILE='@CLAUDE_INSTALLER_FILE@'
CLAUDE_INSTALLER_BYTES='@CLAUDE_INSTALLER_BYTES@'
CLAUDE_INSTALLER_SHA256='@CLAUDE_INSTALLER_SHA256@'

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
verify_file() {
  local file=$1 bytes=$2 expected=$3 size digest
  [[ -f $STAGE/$file ]] || fail "missing staged file: $file"
  size=$(/usr/bin/stat -f '%z' "$STAGE/$file")
  digest=$(/usr/bin/shasum -a 256 "$STAGE/$file" | /usr/bin/awk '{print $1}')
  [[ $size == $bytes && $digest == $expected ]] || fail "staged file did not match its locked size and SHA-256: $file"
}
# BEGIN CLT PAYLOAD HEALTH HELPERS
clt_payload_ready() {
  local developer_directory=$1 work=$2
  [[ -x $developer_directory/usr/bin/git ]] || return 1
  [[ -x $developer_directory/usr/bin/swiftc ]] || return 1
  [[ -x $developer_directory/usr/bin/clang ]] || return 1
  [[ -d $developer_directory/SDKs/MacOSX.sdk ]] || return 1
  "$developer_directory/usr/bin/git" --version >/dev/null 2>&1 || return 1
  "$developer_directory/usr/bin/swiftc" --version >/dev/null 2>&1 || return 1
  print -r -- 'int main(void) { return 0; }' >"$work/clt-readiness.c" || return 1
  "$developer_directory/usr/bin/clang" \
    -isysroot "$developer_directory/SDKs/MacOSX.sdk" \
    "$work/clt-readiness.c" -o "$work/clt-readiness" >/dev/null 2>&1 || return 1
  [[ -x $work/clt-readiness ]] && "$work/clt-readiness" >/dev/null 2>&1
}
clt_needs_reinstall() {
  local installed=$1 version=$2 developer_directory=$3 work=$4
  [[ $installed != $version ]] || ! clt_payload_ready "$developer_directory" "$work"
}
clt_package_needs_install() {
  local installed=$1 version=$2 repair=$3
  [[ $installed != $version ]] || (( repair == 1 ))
}
# END CLT PAYLOAD HEALTH HELPERS

guest_build=$(/usr/bin/sw_vers -buildVersion)
supported=0
for build in $SUPPORTED_GUEST_BUILDS; do
  [[ $guest_build == $build ]] && supported=1
done
(( supported == 1 )) || fail "developer tools are not locked for guest build $guest_build"
[[ $(/usr/bin/uname -m) == arm64 ]] || fail "the locked developer tools require Apple silicon"

installed=$(/usr/sbin/pkgutil --pkg-info com.apple.pkg.CLTools_Executables 2>/dev/null | /usr/bin/awk -F': ' '$1 == "version" {print $2}' || true)
clt_repair=0
if clt_needs_reinstall "$installed" @CLT_EXECUTABLES_RECEIPT_VERSION@ "$CLT_DIRECTORY" "$WORK"; then
  clt_repair=1
  say "CLT receipt or payload needs repair; reinstalling the locked packages"
fi

for entry in $CLT_PACKAGES; do
  fields=(${(s:|:)entry})
  filename=$fields[1]
  receipt=$fields[2]
  version=$fields[3]
  bytes=$fields[4]
  sha=$fields[5]
  verify_file "$filename" "$bytes" "$sha"
  signature=$(/usr/sbin/pkgutil --check-signature "$STAGE/$filename" 2>&1) ||
    fail "could not verify the Apple signature for $filename"
  normalized=$(print -r -- "$signature" | /usr/bin/tr '[:lower:]' '[:upper:]' | /usr/bin/tr -cd 'A-F0-9')
  [[ $signature == *"Status: signed Apple Software"* && $signature == *"$APPLE_SIGNER"* &&
    $normalized == *$APPLE_SIGNER_SHA256* ]] ||
    fail "the Apple signing identity does not match the lock: $filename"
  say "Verified Apple package $filename ($APPLE_SIGNER; $receipt $version)"
  installed=$(/usr/sbin/pkgutil --pkg-info "$receipt" 2>/dev/null | /usr/bin/awk -F': ' '$1 == "version" {print $2}' || true)
  if clt_package_needs_install "$installed" "$version" "$clt_repair"; then
    logged /usr/bin/sudo -n /usr/sbin/installer -pkg "$STAGE/$filename" -target /
  else
    say "Apple package receipt already matches: $receipt $version"
  fi
  installed=$(/usr/sbin/pkgutil --pkg-info "$receipt" 2>/dev/null | /usr/bin/awk -F': ' '$1 == "version" {print $2}' || true)
  [[ $installed == $version ]] || fail "Apple package receipt verification failed: $receipt"
done

logged /usr/bin/sudo -n /usr/bin/xcode-select --switch "$CLT_DIRECTORY"
[[ $(/usr/bin/xcode-select -p) == $CLT_DIRECTORY ]] ||
  fail "Xcode Command Line Tools are not the active developer directory"
[[ -x $CLT_DIRECTORY/usr/bin/git ]] || fail "Apple Git is missing"
[[ -x $CLT_DIRECTORY/usr/bin/swiftc ]] || fail "the Swift compiler is missing"
git_version=$(/usr/bin/git --version) || fail "Git did not run"
say "$git_version"
[[ $(/usr/sbin/pkgutil --pkg-info com.apple.pkg.CLTools_Executables | /usr/bin/awk -F': ' '$1 == "version" {print $2}') == @CLT_EXECUTABLES_RECEIPT_VERSION@ ]] ||
  fail "the CLT executable receipt does not match the lock"
clang=$(/usr/bin/xcrun --find clang) || fail "xcrun could not find clang"
say "xcrun clang=$clang"
cat >"$WORK/hello.c" <<'C'
int main(void) { return 0; }
C
logged /usr/bin/xcrun clang "$WORK/hello.c" -o "$WORK/hello"
logged "$WORK/hello"

verify_file "$GIT_LFS_FILE" "$GIT_LFS_BYTES" "$GIT_LFS_SHA256"
verify_file "$GH_FILE" "$GH_BYTES" "$GH_SHA256"
verify_file "$NODE_FILE" "$NODE_BYTES" "$NODE_SHA256"
verify_file "$CLAUDE_INSTALLER_FILE" "$CLAUDE_INSTALLER_BYTES" "$CLAUDE_INSTALLER_SHA256"
for archive in "$STAGE/$GIT_LFS_FILE" "$STAGE/$GH_FILE"; do
  members=$(/usr/bin/unzip -Z1 "$archive") || fail "could not inspect a pinned tools archive"
  while IFS= read -r member; do
    [[ -n $member && $member != /* && $member != .. && $member != ../* && $member != */../* && $member != */.. ]] ||
      fail "a pinned tools archive has an unsafe path"
  done <<<"$members"
done

node_members=$(/usr/bin/tar -tzf "$STAGE/$NODE_FILE") || fail "could not inspect the pinned Node.js archive"
while IFS= read -r member; do
  [[ -n $member && $member != /* && $member != .. && $member != ../* && $member != */../* && $member != */.. ]] ||
    fail "the pinned Node.js archive has an unsafe path"
done <<<"$node_members"
/usr/bin/tar -xzf "$STAGE/$NODE_FILE" -C "$WORK" || fail "could not unpack Node.js"
[[ -x $WORK/node-v$NODE_VERSION-darwin-arm64/bin/node ]] || fail "the pinned Node.js executable is missing"
/usr/bin/ditto -x -k "$STAGE/$GIT_LFS_FILE" "$WORK" || fail "could not unpack Git LFS"
/usr/bin/ditto -x -k "$STAGE/$GH_FILE" "$WORK" || fail "could not unpack GitHub CLI"
[[ -x $WORK/git-lfs-$GIT_LFS_VERSION/git-lfs ]] || fail "the pinned Git LFS executable is missing"
[[ -x $WORK/gh_${GH_VERSION}_macOS_arm64/bin/gh ]] || fail "the pinned GitHub CLI executable is missing"

TOOLS=/usr/local/libexec/silo-tools/$TOOLS_VERSION
logged /usr/bin/sudo -n /usr/bin/install -d -o root -g wheel -m 0755 "$TOOLS/bin" /usr/local/bin
logged /usr/bin/sudo -n /usr/bin/install -m 0755 "$WORK/git-lfs-$GIT_LFS_VERSION/git-lfs" "$TOOLS/bin/git-lfs"
logged /usr/bin/sudo -n /usr/bin/install -m 0755 "$WORK/gh_${GH_VERSION}_macOS_arm64/bin/gh" "$TOOLS/bin/gh"
logged /usr/bin/sudo -n /bin/ln -sfn "$TOOLS/bin/git-lfs" /usr/local/bin/git-lfs
logged /usr/bin/sudo -n /bin/ln -sfn "$TOOLS/bin/gh" /usr/local/bin/gh
# Node.js LTS (the official tarball, checked against the lock above): node, npm and npx.
logged /usr/bin/sudo -n /bin/rm -rf "$TOOLS/node"
logged /usr/bin/sudo -n /usr/bin/ditto "$WORK/node-v$NODE_VERSION-darwin-arm64" "$TOOLS/node"
for node_tool in node npm npx; do
  logged /usr/bin/sudo -n /bin/ln -sfn "$TOOLS/node/bin/$node_tool" "/usr/local/bin/$node_tool"
done

ZSHENV=$HOME/.zshenv
PATH_LINE='export PATH="/usr/local/bin:$PATH"'
[[ ! -L $ZSHENV && ( ! -e $ZSHENV || -f $ZSHENV ) ]] || fail "the guest zsh environment file is not a regular file"
if ! /usr/bin/grep -Fqx "$PATH_LINE" "$ZSHENV" 2>/dev/null; then
  if [[ ! -e $ZSHENV ]]; then
    print -r -- '# Silo developer tools' >"$ZSHENV"
  elif [[ -s $ZSHENV && $(/usr/bin/tail -c 1 "$ZSHENV" | /usr/bin/wc -l | /usr/bin/tr -d ' ') == 0 ]]; then
    print >>"$ZSHENV"
  fi
  print -r -- "$PATH_LINE" >>"$ZSHENV"
fi

[[ -f $STAGE/setup-github.sh ]] || fail "the shared GitHub credential helper installer is missing"
# macOS sudo keeps HOME, so root would create ~/.local/state/gh in the account home (`gh --version`
# writes its device id there) and later installers could not create ~/.local/share. Root gets its own HOME.
logged /usr/bin/sudo -n /usr/bin/env "HOME=/var/root" "PATH=$PATH" /bin/sh "$STAGE/setup-github.sh"
[[ $(/usr/bin/git config --system --get credential.https://github.com.helper) == /usr/local/libexec/silo-github-credential ]] ||
  fail "the shared GitHub credential helper is not configured"
[[ $(/usr/local/bin/node --version) == "v$NODE_VERSION" ]] || fail "Node.js version verification failed"
/usr/local/bin/npm --version >/dev/null 2>&1 || fail "npm did not run"

# Claude Code, with Anthropic's documented installer (the staged file is the one the lock pins),
# as the working account. The installer checks the binary against Anthropic's release manifest.
# Auto-update stays off so the pinned version holds. Silo never signs in to it.
logged /usr/bin/env "HOME=$HOME" "DISABLE_AUTOUPDATER=1" "PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
  /bin/bash "$STAGE/$CLAUDE_INSTALLER_FILE" "$CLAUDE_VERSION"
[[ -x $HOME/.local/bin/claude ]] || fail "the Claude Code installer did not create ~/.local/bin/claude"
logged /usr/bin/sudo -n /bin/ln -sfn "$HOME/.local/bin/claude" /usr/local/bin/claude
claude_version=$(/usr/bin/env "HOME=$HOME" /usr/local/bin/claude --version) || fail "Claude Code did not run"
[[ $claude_version == "$CLAUDE_VERSION"* ]] || fail "Claude Code version verification failed"

[[ $(/usr/bin/git lfs version) == "git-lfs/$GIT_LFS_VERSION"* ]] || fail "Git LFS version verification failed"
gh_version=$(/usr/local/bin/gh --version | /usr/bin/head -n 1)
[[ $gh_version == "gh version $GH_VERSION"* ]] || fail "GitHub CLI version verification failed"
nonlogin=$(/usr/bin/env -i HOME="$HOME" PATH=/usr/bin:/bin:/usr/sbin:/sbin /bin/zsh -c 'git lfs version && gh --version | head -n 1 && node --version && claude --version') ||
  fail "the tools are not on the managed guest shell PATH"
say "nonlogin_shell=$nonlogin"

lock_digest=$(/usr/bin/shasum -a 256 "$STAGE/developer-tools-lock.json" | /usr/bin/awk '{print $1}')
[[ $lock_digest == @LOCK_SHA256@ ]] || fail "the staged developer tools lock changed"
receipt_dir='/Library/Application Support/Silo'
receipt_tmp="$WORK/developer-tools-receipt.json"
receipt="$receipt_dir/developer-tools-receipt.json"
printf '{\n  "schemaVersion": 1,\n  "guestBuild": "%s",\n  "lockDigest": "%s",\n  "developerDirectory": "/Library/Developer/CommandLineTools",\n  "cltVersion": "%s",\n  "gitVersion": "%s",\n  "gitLfsVersion": "%s",\n  "ghVersion": "%s",\n  "nodeVersion": "%s",\n  "claudeVersion": "%s",\n  "xcodeVersion": null\n}\n' \
  "$guest_build" "$lock_digest" "$CLT_VERSION" "$git_version" "git-lfs/$GIT_LFS_VERSION" "$gh_version" "$NODE_VERSION" "$CLAUDE_VERSION" >"$receipt_tmp"
logged /usr/bin/sudo -n /usr/bin/install -d -o root -g wheel -m 0755 "$receipt_dir"
logged /usr/bin/sudo -n /usr/bin/install -o root -g wheel -m 0644 "$receipt_tmp" "$receipt"
say "Developer tools are ready for $guest_build: $git_version; git-lfs/$GIT_LFS_VERSION; $gh_version; node v$NODE_VERSION; claude $CLAUDE_VERSION; $CLT_VERSION"
/bin/rm -rf "$STAGE"
