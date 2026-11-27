#!/bin/bash
# The Linux analogue of test 08, on a fake HOME: py/08-sshconfig.py's install and restore in every
# case that can happen to ~/.ssh/config (no file, no ~/.ssh, a file, a link to a dotfiles file,
# a file changed during the test, a run cut off by kill -9), then the test itself against a
# throwaway sshd, through ProxyUseFdpass and the relay. The real ~/.ssh is never touched.
set -u
[ "$(uname -s)" = Linux ] || { echo "Linux only: skipped"; exit 0; }
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
tmp="$(mktemp -d /tmp/kit08.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT
P="python3 -I $KIT_ROOT/py/08-sshconfig.py"
fails=0
bad() { echo "FAIL $*"; fails=$((fails + 1)); }
LINE='Include ~/SiloKit/work/08/ssh_config'

scenario() { # name: fresh fake home per scenario
  H="$tmp/$1"; mkdir -p "$H"; S="$H/state.json"; Bk="$H/backup"
}

# 1. A file with content, odd mode, an xattr-free file: back byte for byte, same mode and mtime.
scenario file; mkdir -p "$H/.ssh"; printf 'Host *\n  ForwardAgent yes\n# no newline at end' > "$H/.ssh/config"
chmod 640 "$H/.ssh/config"; touch -d '2020-01-02 03:04:05' "$H/.ssh/config"
cp -p "$H/.ssh/config" "$H/orig"; ino="$(stat -c %i "$H/.ssh/config")"
$P install --config "$H/.ssh/config" --line "$LINE" --state "$S" --backup "$Bk" >/dev/null || bad "file: install"
$P check --config "$H/.ssh/config" --line "$LINE" || bad "file: line not first"
[ "$(stat -c %i "$H/.ssh/config")" = "$ino" ] || bad "file: inode changed"
$P restore --state "$S" | grep -q '"verified": true' || bad "file: restore not verified"
cmp -s "$H/.ssh/config" "$H/orig" || bad "file: bytes differ"
[ "$(stat -c '%a %Y' "$H/.ssh/config")" = "$(stat -c '%a %Y' "$H/orig")" ] || bad "file: mode or mtime differ"

# 2. No file, no ~/.ssh: both made, both gone after.
scenario none
$P install --config "$H/.ssh/config" --line "$LINE" --state "$S" --backup "$Bk" >/dev/null || bad "none: install"
[ "$(head -1 "$H/.ssh/config")" = "$LINE" ] || bad "none: line"
[ "$(stat -c %a "$H/.ssh")" = 700 ] || bad "none: ~/.ssh mode"
$P restore --state "$S" | grep -q '"verified": true' || bad "none: restore"
[ ! -e "$H/.ssh" ] || bad "none: ~/.ssh left behind"

# 3. ~/.ssh/config is a link into a dotfiles folder: the target changes, the link stays.
scenario link; mkdir -p "$H/.ssh" "$H/dotfiles"; printf 'Host a\n  User b\n' > "$H/dotfiles/ssh_config"
ln -s ../dotfiles/ssh_config "$H/.ssh/config"; cp "$H/dotfiles/ssh_config" "$H/orig"
$P install --config "$H/.ssh/config" --line "$LINE" --state "$S" --backup "$Bk" | grep -q '"was_symlink": true' || bad "link: not seen"
[ -L "$H/.ssh/config" ] || bad "link: replaced by a file"
[ "$(head -1 "$H/dotfiles/ssh_config")" = "$LINE" ] || bad "link: target not changed"
r="$($P restore --state "$S")"
echo "$r" | grep -q '"symlink_kept": true' || bad "link: $r"
cmp -s "$H/dotfiles/ssh_config" "$H/orig" || bad "link: bytes differ"

# 4. Something appended to the file during the test: its change stays, only the line goes.
scenario changed; mkdir -p "$H/.ssh"; printf 'Host x\n' > "$H/.ssh/config"
$P install --config "$H/.ssh/config" --line "$LINE" --state "$S" --backup "$Bk" >/dev/null
printf 'Host added-by-an-app\n' >> "$H/.ssh/config"
r="$($P restore --state "$S")"
echo "$r" | grep -q 'changed during the test' || bad "changed: $r"
[ "$(cat "$H/.ssh/config")" = "$(printf 'Host x\nHost added-by-an-app')" ] || bad "changed: content $(cat "$H/.ssh/config")"
[ -f "$Bk.changed-during-test" ] || bad "changed: copy not kept"

# 5. A run killed with kill -9 after install: the next run's start restores it.
scenario killed; mkdir -p "$H/.ssh" "$H/work"; printf 'Host k\n' > "$H/.ssh/config"; cp "$H/.ssh/config" "$H/orig"
$P install --config "$H/.ssh/config" --line "$LINE" --state "$H/work/ssh_config.state.json" --backup "$H/work/ssh_config.backup" >/dev/null
$P restore --state "$H/work/ssh_config.state.json" >/dev/null
cmp -s "$H/.ssh/config" "$H/orig" || bad "killed: not restored"

# 6. The test itself, on a fake HOME, against a throwaway sshd.
H="$tmp/run"; mkdir -p "$H/.ssh"
printf 'Host *\n  ServerAliveInterval 30\n' > "$H/.ssh/config"; cp -p "$H/.ssh/config" "$tmp/run-orig"
export HOME="$H" KIT_WORK_ROOT="$H/SiloKit" KIT_ID=08 KIT_WORK="$H/SiloKit/work/08" KIT_OUT="$tmp/results.jsonl" KIT_FAKE=1 KIT_YES=1
mkdir -p "$KIT_WORK" "$KIT_WORK_ROOT/bin"
cc -O2 -Wall -o "$KIT_WORK_ROOT/bin/sshproxy" "$KIT_ROOT/c/sshproxy.c" || bad "sshproxy build"
: > "$KIT_OUT"
bash "$KIT_ROOT/tests/08-codex-claude-ssh-config.sh" < /dev/null
if grep -q '"kind":"skip"' "$KIT_OUT"; then
  echo "test 08's run skipped: $(grep '"kind":"skip"' "$KIT_OUT")"
else
  cmp -s "$H/.ssh/config" "$tmp/run-orig" || bad "run: ~/.ssh/config differs after the test"
  for k in 'ssh_cli.silo-kit-main","value":"ok' 'ssh_cli.silo-kit-main-relay","value":"ok' 'proxyusefdpass yes' 'verified.": true'; do
    grep -q "$k" "$KIT_OUT" || bad "run: no $k in the results"
  done
  grep '"kind":"fact"' "$KIT_OUT" | grep -E 'ssh_cli|proxy_log|restore' | cut -c1-300
fi
[ "$fails" = 0 ] && echo "test 08 analogue: all passed"
exit $((fails > 0))
