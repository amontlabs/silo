#!/usr/bin/env bash
# Tests merge-batch's gate (green check per branch, --force, the check skip), the board update, and the
# cancellation of a suite for an older commit. Temporary git repos and stand-in tools only.
set -uo pipefail
A=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
W=$(mktemp -d "${TMPDIR:-/tmp}/merge-gate-test.XXXXXX"); W=$(cd "$W" && pwd -P); trap 'rm -rf "$W"' EXIT
export TMPDIR=$W/tmp; mkdir -p "$TMPDIR"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
unset BOARD BOARD_INTEGRATION SILO_BUILD_STATE
N=0; BAD=0
ok() { N=$((N+1)); if [ "$2" = 0 ]; then echo "PASS $1"; else echo "FAIL $1 ${3:-}"; BAD=$((BAD+1)); fi; }
t() { if eval "$2"; then ok "$1" 0; else ok "$1" 1 "${3:-}"; fi; }

R=$W/repo; mkdir -p $R/app/SiloUI $R/tools/agent; git -C $R init -q -b main
echo a >$R/app/SiloUI/f; printf '#!/bin/sh\necho "$@" >>%s/check.calls\n' $W >$R/tools/agent/check; chmod +x $R/tools/agent/check
echo target >>$R/.git/info/exclude
git -C $R add -A; git -C $R commit -qm init
MAIN0=$(git -C $R rev-parse HEAD)
mkb() {   # mkb NAME FILE : a branch off main adding FILE, checked out in its own worktree $W/wt-NAME
  git -C $R branch -q $1 $MAIN0; git -C $R worktree add -q $W/wt-$1 $1
  echo $1 >$W/wt-$1/$2; git -C $W/wt-$1 add -A; git -C $W/wt-$1 commit -qm $1
}
green() {  # green BRANCH [BASE] : what check records for the tree of BRANCH, in its worktree
  local d=$W/wt-$1/app/SiloUI/target/agent-check/green; mkdir -p $d
  echo "${2:-$MAIN0}" >$d/tree-$(git -C $R rev-parse $1^{tree})
}
mb() { ( cd $R && python3 -I $A/merge-batch --no-suite "$@" ) >$W/out 2>&1; }
hd() { git -C $R rev-parse HEAD; }
reset() { git -C $R reset -q --hard $MAIN0; rm -f $W/check.calls; }

mkb b1 f1; mkb b2 f2; mkb b3 f3
mb b1; rc=$?
t "a branch without a recorded green check is refused and nothing is merged" '[ $rc = 4 ] && [ "$(hd)" = $MAIN0 ] && grep -q "REFUSED b1" $W/out' "$(cat $W/out)"
mb --force b1; rc=$?
t "--force merges it anyway, with a note, and the foreground check runs" '[ $rc = 0 ] && [ "$(hd)" != $MAIN0 ] && grep -q "merging anyway" $W/out && grep -q -- "--base $MAIN0" $W/check.calls' "$(cat $W/out)"
reset
green b1
mb b1; rc=$?
t "a branch whose tree check recorded as green is merged and the merged tree needs no second check" '[ $rc = 0 ] && [ "$(hd)" != $MAIN0 ] && grep -q "check skipped" $W/out && [ ! -e $W/check.calls ]' "$(cat $W/out)"
reset
green b1; green b2
mb b1 b2; rc=$?
t "two green branches combine into a new tree: the foreground check runs on the union" '[ $rc = 0 ] && grep -q -- "--base $MAIN0" $W/check.calls' "$(cat $W/out)"
reset
green b1; green b3 "$(git -C $R rev-parse b1)"
mb b3; rc=$?
t "a green check whose base does not cover the branch is refused" '[ $rc = 4 ] && grep -q "does not cover" $W/out' "$(cat $W/out)"
mb b1 b3; rc=$?
t "a base inside an earlier branch of the batch is accepted (stacked)" '[ $rc = 0 ]' "$(cat $W/out)"
reset
mb b1; rc=$?
t "a branch already contained needs no check" '[ $rc = 0 ]'
git -C $R merge -q --no-ff -m m b1 >/dev/null 2>&1; mb b1; rc=$?
t "an already contained branch is skipped by the gate" '[ $rc = 0 ] && grep -q "already contained" $W/out' "$(cat $W/out)"
reset

# board: state=merged for each branch actually merged, quietly nothing without a board
printf 'item\tbranch\tbase\tdepends_on\tworktree\tstate\tnote\ttemp\n' >$W/board.tsv
$A/board --file $W/board.tsv add it1 --branch b1 >/dev/null; $A/board --file $W/board.tsv add it3 --branch b3 >/dev/null
green b1; green b3
BOARD=$W/board.tsv mb b1; rc=$?
t "the board row of a merged branch gets state=merged, the other rows stay" '[ $rc = 0 ] && [ "$($A/board --file $W/board.tsv get it1 state)" = merged ] && [ "$($A/board --file $W/board.tsv get it3 state)" = planned ]' "$(cat $W/out)"
reset
mkdir -p $R.state; cp $W/board.tsv $R.state/board.tsv; $A/board --file $R.state/board.tsv set it1 state=committed
mb b1
t "the board defaults to <checkout>.state/board.tsv" '[ "$($A/board --file $R.state/board.tsv get it1 state)" = merged ]' "$(cat $W/out)"
reset; rm -rf $R.state
mb b1; rc=$?
t "no board file: skipped quietly" '[ $rc = 0 ] && ! grep -qi board $W/out' "$(cat $W/out)"
reset

# a branch whose merge conflicts is not marked merged
git -C $R branch -q c1 $MAIN0; git -C $R worktree add -q $W/wt-c1 c1; echo other >$W/wt-c1/app/SiloUI/f; git -C $W/wt-c1 commit -qam c1
echo mainside >$R/app/SiloUI/f; git -C $R commit -qam mainside; MAIN1=$(hd)
$A/board --file $W/board.tsv add itc --branch c1 >/dev/null; green c1 "$MAIN0"
BOARD=$W/board.tsv mb c1; rc=$?
t "a conflicting branch is aborted and stays unmerged on the board" '[ $rc = 2 ] && [ "$($A/board --file $W/board.tsv get itc state)" = planned ]' "$(cat $W/out)"
git -C $R reset -q --hard $MAIN0

# suites: starting one cancels a running suite for an older commit
cat >$W/drive.py <<'P'
import importlib.machinery, importlib.util, sys
l = importlib.machinery.SourceFileLoader("mb", sys.argv[1]); s = importlib.util.spec_from_loader("mb", l)
m = importlib.util.module_from_spec(s); l.exec_module(m)
m.ROOT = sys.argv[2]; m.SUITE = sys.argv[4]
m.start_suite(sys.argv[3], [])
P
drive() { python3 -I $W/drive.py $A/merge-batch $R "$1" 'sleep 3' >$W/drv 2>&1; }
D=$R/app/SiloUI/target/agent; mkdir -p $D
fakesuite() {   # a suite-looking process (own process group, command line names the base) for commit $1
  python3 -I - "$1" $D $W <<'P' &
import subprocess, sys
sha, d, w = sys.argv[1:]
p = subprocess.Popen(["bash", "-c", "sleep 60; :", "x", d + "/full-suite." + sha], start_new_session=True,
                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
open(d + "/full-suite.%s.running" % sha, "w").write(str(p.pid))
P
  wait; sleep 0.3
}
git -C $R checkout -q -b newer; echo n >$R/app/SiloUI/n; git -C $R add -A; git -C $R commit -qm newer; NEW=$(hd)
fakesuite $MAIN0; OLDPID=$(cat $D/full-suite.$MAIN0.running)
drive $NEW; rc=$?
t "a suite for an older commit is cancelled and marked superseded" '[ $rc = 0 ] && ! kill -0 $OLDPID 2>/dev/null && [ "$(head -2 $D/full-suite.$MAIN0.done | tr "\n" " ")" = "SUPERSEDED superseded by $NEW " ] && [ ! -e $D/full-suite.$MAIN0.running ]' "$(cat $W/drv) $(cat $D/full-suite.$MAIN0.done 2>&1)"
sleep 3.5; rm -f $D/full-suite.$NEW.*
fakesuite $NEW; NEWPID=$(cat $D/full-suite.$NEW.running)
drive $MAIN0; rc=$?
t "a suite for a commit that is not older is never touched; the start is refused" '[ $rc = 3 ] && kill -0 $NEWPID 2>/dev/null && [ ! -e $D/full-suite.$MAIN0.running ]' "$(cat $W/drv)"
kill -- -$NEWPID 2>/dev/null; rm -f $D/full-suite.*
echo $$ >$D/full-suite.$MAIN0.running
drive $NEW; rc=$?
t "a guard naming a process that is not a suite's group leader is not signalled" '[ $rc = 3 ] && kill -0 $$ && grep -q "not touching" $W/drv' "$(cat $W/drv)"
rm -f $D/full-suite.*
# --last prefers a real verdict over a superseded one
printf 'PASS\nsha a\n' >$D/full-suite.aaa.done; sleep 1; printf 'SUPERSEDED\nsuperseded by b\nsha c\n' >$D/full-suite.ccc.done
( cd $R && python3 -I $A/merge-batch --last ) >$W/last 2>&1
t "--last shows the newest verdict that is not superseded" 'head -1 $W/last | grep -q "^PASS  full-suite.aaa"' "$(cat $W/last)"

echo "$N tests, $BAD failed"; [ $BAD = 0 ]
