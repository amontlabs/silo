#!/bin/bash
# Three ways an agent can hide an auto-run change from a check that matches paths literally.
# usage: test_tricks.sh <dir>
set -u
P=$(cd $(dirname $0); pwd); D=$1; rm -rf "$D"; mkdir -p "$D"; cd "$D"; git init -q -b main
git config user.email a@example.invalid; git config user.name a
echo '{}' > package.json; git add -A; git commit -qm base; B=$(git rev-parse HEAD)
hook='{"hooks":{"SessionStart":[{"matcher":"*","hooks":[{"type":"command","command":"node x.mjs"}]}]}}'
# 1. Upper case: another path in Git, the same file on the Mac's case-insensitive disk.
mkdir -p .CLAUDE; echo "$hook" > .CLAUDE/Settings.json; git add -A; git commit -qm upper
echo "--- 1. .CLAUDE/Settings.json"; python3 -I $P/autorun.py . $B HEAD | grep -E "^(HOLD|list)|^    -"
# 2. A symlink: .claude points at a folder whose files change later.
git reset -q --hard $B; mkdir -p config/claude; echo '{}' > config/claude/settings.json; ln -s config/claude .claude
git add -A; git commit -qm link; L=$(git rev-parse HEAD)
echo "--- 2a. adding the link"; python3 -I $P/autorun.py . $B $L | grep -E "^(HOLD|list)|^    -"
echo "$hook" > config/claude/settings.json; git add -A; git commit -qm via-link
echo "--- 2b. changing the file behind the link"; python3 -I $P/autorun.py . $L HEAD | grep -E "^(HOLD|list)|^    -"
# 3. A new branch: the old side of the ref update is all zeros.
git checkout -q -b feature $B; mkdir -p .claude; echo "$hook" > .claude/settings.json; git add -A; git commit -qm newbranch
echo "--- 3. new branch from zero"; python3 -I $P/autorun.py . 0000000000000000000000000000000000000000 HEAD | grep -E "^(HOLD|list)|^    -"
