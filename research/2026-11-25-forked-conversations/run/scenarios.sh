#!/usr/bin/env bash
# Every fork case, for one program: cc (Claude Code) or cx (Codex). Writes the traffic to
# $OUTROOT/<program>/requests.jsonl and the truth (which step was sent from where, on which branch)
# to steps.jsonl. Usage: run/scenarios.sh cc|cx
set -euo pipefail
PROG=$1
export PORTS="main=8101,fork=8102,fork2=8103,old=8104,forkid=8105,main2=8106"
R=$(cd "$(dirname "$0")/.." && pwd)
source "$R/run/lib.sh"; source "$R/run/standin.sh"
OUTROOT=${OUTROOT:-/tmp/forkruns/out}
OUT="$OUTROOT/$PROG"; rm -rf "$OUT"; mkdir -p "$OUT"
export C=/tmp/forkruns/C-$PROG; rm -rf "$C"
start_standin "$OUT"
STEPS="$OUT/steps.jsonl"

# step computer branch prompt [resume-args...]: one user turn; prints the session id it ran in.
step() {
  local comp=$1 branch=$2 prompt=$3; shift 3
  local before; before=$(wc -l < "$OUT/requests.jsonl" 2>/dev/null || echo 0)
  local out sid
  if [ "$PROG" = cc ]; then
    out=$(cc "$comp" "$@" "$prompt")
    sid=$(printf '%s' "$out" | python3 -c 'import json,sys;print(json.loads(sys.stdin.read().strip().splitlines()[-1])["session_id"])')
  else
    out=$(cx "$comp" "$@" "$prompt" 2>>"$OUT/program.err")
    sid=$(printf '%s' "$out" | python3 -c 'import json,sys
for l in sys.stdin:
  try: o=json.loads(l)
  except Exception: continue
  if o.get("type")=="thread.started": print(o["thread_id"])')
  fi
  sleep 0.2
  local after; after=$(wc -l < "$OUT/requests.jsonl")
  printf '{"computer":"%s","branch":"%s","prompt":"%s","session":"%s","first":%d,"last":%d,"how":"%s"}\n' \
    "$comp" "$branch" "$prompt" "$sid" "$((before+1))" "$after" "$*" >> "$STEPS"
  echo "$sid"
}
resume() { if [ "$PROG" = cc ]; then echo "--resume $1"; else echo "resume $1"; fi; }
own_fork() { if [ "$PROG" = cc ]; then echo "--resume $1 --fork-session"; else echo "fork $1"; fi; }
# Codex's resume and fork are subcommands of exec, placed before the prompt.
run() { local comp=$1 branch=$2 prompt=$3 how=${4:-}; if [ -n "$how" ]; then step "$comp" "$branch" "$prompt" $how; else step "$comp" "$branch" "$prompt"; fi; }

new_computer main

# A. Main works two turns of one conversation.
S=$(run main A "Write the login page, part 1.")
run main A "Write the login page, part 2." "$(resume "$S")" >/dev/null
# A checkpoint of main after two turns, kept for case D.
copy_disk "$C/main" "$C/checkpoint"

# B. A fork of main, same disk and same machine identity (a copy), takes the conversation up.
fork_computer main fork
run fork B "Try the risky refactor, part 1." "$(resume "$S")" >/dev/null
run fork B "Try the risky refactor, part 2." "$(resume "$S")" >/dev/null
# Main goes on with the same conversation.
run main A "Write the login page, part 3." "$(resume "$S")" >/dev/null

# C. A fork with a new machine identity (`fork-identity`) takes the same conversation up.
fork_computer main forkid new-identity
run forkid C "Try another refactor." "$(resume "$S")" >/dev/null

# D. A fork of the checkpoint taken after two turns: it goes back to turn 2 while main is at 3.
copy_disk "$C/checkpoint" "$C/old"
run old D "From the checkpoint, try a third way." "$(resume "$S")" >/dev/null

# E. A fork of a fork.
fork_computer fork fork2
run fork2 E "On the fork of the fork, go on." "$(resume "$S")" >/dev/null

# F. The program's own fork of the conversation, inside main (Claude Code --fork-session, codex exec fork).
F=$(run main F "Branch inside main: try plan B." "$(own_fork "$S")")
run main F "Branch inside main: plan B, part 2." "$(resume "$F")" >/dev/null
run main A "Write the login page, part 4." "$(resume "$S")" >/dev/null

# G. The fork becomes main: the fork's disk is now main's (`fork-replacing-main`), on main's port.
# main2 stands for main after the replacement: a new port would hide it, so it reuses main's.
rm -rf "$C/main.old"; mv "$C/main" "$C/main.old"; copy_disk "$C/fork" "$C/main"
run main G "Now main again, go on from the refactor." "$(resume "$S")" >/dev/null

stop_standin
echo "done $PROG: $(wc -l < "$OUT/requests.jsonl") requests, $(wc -l < "$STEPS") steps"
