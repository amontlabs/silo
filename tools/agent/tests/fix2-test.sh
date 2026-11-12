#!/usr/bin/env bash
# Tests merge-batch (start guard, suite worktree), board restack after a rewind, and clean's handling of
# a symlink named with a trailing "/". Temporary git repos only; no cargo, node or engine is started.
set -uo pipefail
# Start from a clean environment for every variable the tools read.
unset BOARD BOARD_INTEGRATION SILO_INTEGRATION_BRANCH SILO_AGENT_ITEM SILO_AGENT_CACHE RUNTIME_PREPARE_CMD SILO_BIN SILO_RUNTIME_TARGET LIVE_D LIVE_LINUX_HOST LIVE_SHORT_BASE LIVE_SSH LIVE_STEP
A=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
W=$(mktemp -d "${TMPDIR:-/tmp}/fix2-test.XXXXXX"); W=$(cd "$W" && pwd -P); trap 'rm -rf "$W"' EXIT
export TMPDIR=$W/tmp; mkdir -p "$TMPDIR"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
N=0; BAD=0
ok() { N=$((N+1)); if [ "$2" = 0 ]; then echo "PASS $1"; else echo "FAIL $1 ${3:-}"; BAD=$((BAD+1)); fi; }

R=$W/repo; mkdir -p $R/app/SiloUI; git -C $R init -q -b main; echo a >$R/app/SiloUI/f; git -C $R add -A; git -C $R commit -qm init
mkdir -p $R/app/SiloUI/node_modules
SHA=$(git -C $R rev-parse HEAD)

# merge-batch: python driver with a fast stand-in for the suite script
cat >$W/drive.py <<'P'
import importlib.machinery, importlib.util, sys
l = importlib.machinery.SourceFileLoader("mb", sys.argv[1]); s = importlib.util.spec_from_loader("mb", l)
m = importlib.util.module_from_spec(s); l.exec_module(m)
m.ROOT = sys.argv[2]; m.SUITE = sys.argv[4]
m.start_suite(sys.argv[3], [])
P
drive() { python3 -I $W/drive.py $A/merge-batch $R $SHA "$1"; }
G=$R/app/SiloUI/target/agent/full-suite.$SHA.running

mkdir -p $R/app/SiloUI/target/agent; echo 999999 >$G
drive 'sleep 4' >/dev/null 2>&1; rc=$?
ok "a start guard of a dead pid is replaced" $([ $rc = 0 ] && [ "$(cat $G)" != 999999 ] && echo 0 || echo 1)
drive 'sleep 4' >/dev/null 2>&1; rc=$?
ok "a start guard of a live suite refuses a second start" $([ $rc = 3 ] && echo 0 || echo 1)
kill "$(cat $G)" 2>/dev/null; sleep 0.3
rm -f $G; echo 999999 >$G; rm -f $W/r.*
for i in 1 2 3 4 5 6; do ( drive 'sleep 4' >/dev/null 2>&1; echo $? >$W/r.$i ) & done; wait
ok "six starters on a stale guard: one launches, five are refused" $([ "$(cat $W/r.* | grep -c '^0$')" = 1 ] && [ "$(cat $W/r.* | grep -c '^3$')" = 5 ] && echo 0 || echo 1)
ok "each launch gets its own suite folder" $([ "$(ls -d $TMPDIR/silo-agent/suite-* | wc -l | tr -d ' ')" -ge 2 ] && echo 0 || echo 1)
rm -f $G; rm -rf $TMPDIR/silo-agent

# the real suite script: a path that is already a worktree is never removed by a second launch
SUITE=$(python3 -I - $A/merge-batch <<'P'
import importlib.machinery, importlib.util, sys
l = importlib.machinery.SourceFileLoader("mb", sys.argv[1]); s = importlib.util.spec_from_loader("mb", l)
m = importlib.util.module_from_spec(s); l.exec_module(m); print(m.SUITE)
P
)
mkdir -p $W/bin; for c in cargo npx npm python3; do printf '#!/bin/sh\nexit 0\n' >$W/bin/$c; chmod +x $W/bin/$c; done
git -C $R worktree add -q --detach $W/first $SHA; echo keep >$W/first/marker
B=$R/app/SiloUI/target/agent/full-suite.$SHA
echo $$ >$B.running
PATH=$W/bin:$PATH bash -c "$SUITE" x $R $B $SHA $W/first >/dev/null 2>&1
ok "a second launch on a used worktree path fails without removing it" $([ -e $W/first/marker ] && head -1 $B.done | grep -q FAIL && echo 0 || echo 1)
ok "its start guard of another process is left alone" $([ -e $B.running ] && echo 0 || echo 1)
rm -f $B.done $B.running; mkdir $W/second
PATH=$W/bin:$PATH bash -c "$SUITE" x $R $B $SHA $W/second >/dev/null 2>&1
ok "a launch on its own folder passes and removes the folder" $([ "$(head -1 $B.done)" = PASS ] && [ ! -e $W/second ] && echo 0 || echo 1 "$(cat $B.done)")
grep -q 'test:release' <<<"$SUITE"; ok "the full suite runs npm run test:release" $?

PY=$(command -v python3)   # the suite stubs shadow python3 on PATH
# merge-batch --suite: no merge, same branch checks, suite on HEAD
rm -f $B.done $B.running $B.log
( cd $R && PATH=$W/bin:$PATH $PY $A/merge-batch --suite ) >$W/ms 2>&1; rc=$?
for _ in $(seq 100); do [ -e $B.done ] && break; sleep 0.2; done
ok "merge-batch --suite starts the suite for HEAD without merging" $([ $rc = 0 ] && grep -q "started in the background on ${SHA:0:9}" $W/ms && [ "$(head -1 $B.done)" = PASS ] && [ "$(git -C $R rev-parse HEAD)" = $SHA ] && echo 0 || echo 1 "$(cat $W/ms)")
git -C $R checkout -q -b other
( cd $R && $A/merge-batch --suite ) >$W/ms 2>&1; rc=$?
ok "merge-batch --suite refuses on a branch other than the integration branch" $([ $rc != 0 ] && grep -q "not main" $W/ms && echo 0 || echo 1)
git -C $R checkout -q --detach
( cd $R && $A/merge-batch --suite ) >$W/ms 2>&1; rc=$?
ok "merge-batch --suite refuses on a detached HEAD" $([ $rc != 0 ] && grep -q detached $W/ms && echo 0 || echo 1)
git -C $R checkout -q main; git -C $R branch -q -D other
( cd $R && $A/merge-batch --suite x ) >/dev/null 2>&1; ok "merge-batch --suite refuses branch arguments" $([ $? = 2 ] && echo 0 || echo 1)
rm -f $B.done $B.log $B.merged

# board restack after the parent was rewound
git -C $R worktree remove --force $W/first
BR=$W/brepo; mkdir $BR; git -C $BR init -q -b main; echo 0 >$BR/f; git -C $BR add -A; git -C $BR commit -qm m0
git -C $BR checkout -q -b P; echo p1 >$BR/p1; git -C $BR add -A; git -C $BR commit -qm p1
git -C $BR checkout -q -b C; echo c1 >$BR/c1; git -C $BR add -A; git -C $BR commit -qm c1
git -C $BR checkout -q P; echo p2 >$BR/p2; git -C $BR add -A; git -C $BR commit -qm p2
git -C $BR checkout -q C; git -C $BR rebase -q P >/dev/null 2>&1   # C now holds p2
git -C $BR checkout -q main; git -C $BR branch -q -f P P~1       # P rewound to p1
export BOARD=$W/board.tsv BOARD_INTEGRATION=main
( cd $BR && $A/board add P --branch P && $A/board add C --branch C --base P --depends P && $A/board set P state=committed && $A/board set C state=committed ) >/dev/null 2>&1
( cd $BR && $A/board restack P ) >$W/rs 2>&1; rc=$?
LOG=$(git -C $BR log --format=%s C)
ok "restack after a parent was rewound drops the commit the parent no longer has" $([ $rc = 0 ] && ! grep -qx p2 <<<"$LOG" && grep -qx c1 <<<"$LOG" && echo 0 || echo 1)

# clean: a symlink named with a trailing slash is removed itself, its target survives
CR=$W/crepo; mkdir $CR; git -C $CR init -q -b main; echo 0 >$CR/f; git -C $CR add -A; git -C $CR commit -qm m0
git -C $CR checkout -q -b item-x; git -C $CR checkout -q main; git -C $CR merge -q item-x 2>/dev/null
mkdir -p $TMPDIR/precious; echo keep >$TMPDIR/precious/file; ln -s $TMPDIR/precious $TMPDIR/lnk
export BOARD=$W/cboard.tsv
( cd $CR && $A/board add x --branch item-x --temp "$TMPDIR/lnk/" && $A/board set x state=merged ) >/dev/null 2>&1
( cd $CR && $A/clean --yes x ) >$W/cl 2>&1
ok "clean removes a symlink given with a trailing slash and keeps its target" $([ ! -L $TMPDIR/lnk ] && [ -e $TMPDIR/precious/file ] && echo 0 || echo 1 "$(cat $W/cl)")

echo "$N tests, $BAD failed"; [ $BAD = 0 ]
