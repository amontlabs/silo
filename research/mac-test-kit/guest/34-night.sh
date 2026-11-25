#!/bin/sh
# Runs inside a macOS guest as the provisioned user (test 34): one agent program working on its own
# project until a deadline, launched again with its conversation resumed each time it exits.
#   34-night.sh PROGRAM PHASE SECONDS LAUNCH_SECONDS PAUSE_SECONDS
# PROGRAM is claude or codex. Writes ~/silokit/34/PHASE/PROGRAM/: launches.tsv (one line a launch:
# number, start, end, exit status, the conversation id the program gave, new or resume and how),
# launch-N.jsonl (the program's own stream of events) and launch-N.err. A launch that ends within a
# minute is followed by a pause (PAUSE_SECONDS, doubled each time, at most 15 minutes), so a refused
# plan does not become a loop. ~/silokit/34/stop ends it after the current launch.
set -u
prog="$1"; phase="$2"; secs="$3"; cap="$4"; pause0="${5:-60}"
B="$HOME/silokit/34"
D="$B/$phase/$prog"; P="$B/$phase/$prog-project"
mkdir -p "$D" "$P" && cd "$P" || exit 1
end=$(( $(date +%s) + secs ))
FIRST="You are working alone through the night in this empty folder; nobody will answer questions. Build a small but real command-line program in Perl 5 using only core modules: a plain-text task tracker (add, list, done, search, due dates, priorities), with a test suite under t/ run by prove. Work in small steps and run the tests after each. When it works, keep improving it: more features, edge cases, a README.md, refactoring, always with the tests passing. Do not stop to ask; decide yourself."
NEXT="Continue the work in this folder: read README.md and the tests, run prove, then pick the next most valuable improvement, make it with its tests, and keep going."

# limit SECONDS cmd...: ends cmd after SECONDS (macOS has no timeout command); status 124 then.
limit() { _n="$1"; shift; "$@" & _p=$!; ( sleep "$_n"; touch "$HOME/.silokit-limit-$_p"; kill -TERM "$_p" 2>/dev/null || rm -f "$HOME/.silokit-limit-$_p"; sleep 3; kill -KILL "$_p" 2>/dev/null ) >/dev/null 2>&1 & _w=$!; wait "$_p"; _r=$?; kill "$_w" 2>/dev/null; if [ -f "$HOME/.silokit-limit-$_p" ]; then rm -f "$HOME/.silokit-limit-$_p"; _r=124; fi; return "$_r"; }

idof() { # the conversation id a launch's own events name
  case "$prog" in
    claude) sed -n 's/.*"session_id": *"\([^"]*\)".*/\1/p' "$1" | tail -1 ;;
    codex) sed -n 's/.*"thread_id": *"\([^"]*\)".*/\1/p' "$1" | tail -1 ;;
  esac
}

# Codex with its flags before or after "resume". Only run through limit: exec makes Codex the
# process limit ends.
cx() { exec codex exec --json --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox "$@"; }
cx_after() { _i="$1"; shift; exec codex exec resume "$_i" --json --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox "$@"; }

n=0; id=""; pause="$pause0"
while [ "$(date +%s)" -lt "$end" ] && [ ! -f "$B/stop" ]; do
  n=$((n + 1))
  left=$(( end - $(date +%s) )); c="$cap"; [ "$left" -lt "$c" ] && c="$left"
  out="$D/launch-$n.jsonl"; err="$D/launch-$n.err"
  t0=$(date +%s)
  if [ -z "$id" ]; then
    kind=new
    case "$prog" in
      claude) limit "$c" claude -p "$FIRST" --output-format stream-json --verbose --dangerously-skip-permissions > "$out" 2> "$err" ;;
      codex) limit "$c" cx "$FIRST" > "$out" 2> "$err" ;;
    esac
    rc=$?
  else
    kind=resume
    case "$prog" in
      claude) limit "$c" claude -p "$NEXT" --resume "$id" --output-format stream-json --verbose --dangerously-skip-permissions > "$out" 2> "$err"; rc=$? ;;
      codex)
        limit "$c" cx resume "$id" "$NEXT" > "$out" 2> "$err"; rc=$?
        if [ "$rc" = 2 ] && [ -z "$(idof "$out")" ]; then
          kind=resume-flags-after
          limit "$c" cx_after "$id" "$NEXT" > "$out" 2> "$err"; rc=$?
        fi ;;
    esac
  fi
  t1=$(date +%s)
  seen="$(idof "$out")"
  [ -n "$seen" ] && id="$seen"
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$n" "$t0" "$t1" "$rc" "$seen" "$kind" >> "$D/launches.tsv"
  if [ $((t1 - t0)) -lt 60 ] && [ "$(date +%s)" -lt "$end" ]; then
    w="$pause"; left=$(( end - $(date +%s) )); [ "$left" -lt "$w" ] && w="$left"
    sleep "$w"
    pause=$((pause * 2)); [ "$pause" -gt 900 ] && pause=900
  else
    pause="$pause0"
  fi
done
echo "$n" > "$D/finished"
