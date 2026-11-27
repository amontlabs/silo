#!/bin/sh
# Every Git measurement in the README, appended to out/git.jsonl. HOSTED is a repository URL and
# BRANCH a branch of it this run may write; without them, only the local runs.
cd "$(dirname "$0")"
for m in 1 4 12; do
  for rtt in 0 50 150; do python3 gitbench.py local $rtt $m >> out/git.jsonl; done
done
python3 gitbench.py local 150 4 5 >> out/git.jsonl
python3 gitbench.py local 150 12 10 >> out/git.jsonl
if [ -n "$HOSTED" ]; then
  for m in 1 4 12; do python3 gitbench.py hosted "$HOSTED" "$BRANCH" $m >> out/git.jsonl; done
  python3 gitbench.py hosted "$HOSTED" "$BRANCH" 4 5 >> out/git.jsonl
  python3 gitbench.py hosted "$HOSTED" "$BRANCH" 12 10 >> out/git.jsonl
fi
