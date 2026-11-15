#!/usr/bin/env bash
# Tests the live runner with a fake check: pass, fail, resume, --from, interrupt, offline Linux,
# skip, failing checks, wait on a dead runner, short paths. Touches no engine.
set -uo pipefail
# Start from a clean environment for every variable the tools read.
unset BOARD BOARD_INTEGRATION SILO_INTEGRATION_BRANCH SILO_AGENT_ITEM SILO_AGENT_CACHE RUNTIME_PREPARE_CMD SILO_BIN SILO_RUNTIME_TARGET LIVE_D LIVE_LINUX_HOST LIVE_SHORT_BASE LIVE_SSH LIVE_STEP ATELIER_TMP LIVE_LOCK_DIR LIVE_HELD
LIVE_LIB_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd); LIVE=$LIVE_LIB_DIR/run
W=$(mktemp -d "${TMPDIR:-/tmp}/live-test.XXXXXX"); trap 'rm -rf "$W"' EXIT
export LIVE_D=$W/d LIVE_LOCK_DIR=$W/locks; mkdir -p "$LIVE_D"  # the machine-wide locks live in the test folder: no real lock is touched
N=0; BAD=0
ok() { N=$((N+1)); if [ "$2" = 0 ]; then echo "PASS $1"; else echo "FAIL $1 ${3:-}"; BAD=$((BAD+1)); fi; }
verdict() { sed -n "s/^$2: //p" "$1/VERDICT" 2>/dev/null; }
cat >"$LIVE_D/fake.sh" <<'C'
CHECK_DESC="fake check for the runner tests"
STEPS=(one two three)
step_one() { echo ran >>"$STATE/count-one"; live_set made "$(live_computer)"; }
step_two() { echo ran >>"$STATE/count-two"; [ -e "$STATE/break-two" ] && { echo boom; return 1; }; [ -e "$STATE/hang-two" ] && sleep 30; return 0; }
step_three() { echo ran >>"$STATE/count-three"; live_check "marker" "$([ -e "$STATE/bad-check" ] && echo 1 || echo 0)" "detail"; if [ -e "$STATE/skip-three" ]; then live_skip "no key"; fi; }
cleanup() { echo cleaned >>"$STATE/cleaned"; }
C
cnt() { [ -f "$1" ] && wc -l <"$1" | tr -d ' ' || echo 0; }

S=$W/s1; mkdir -p $S; touch $S/break-two
$LIVE fake --state $S >$W/out 2>&1; rc=$?
ok "failing step exits 1" $([ $rc = 1 ] && echo 0 || echo 1)
ok "verdict fail names step two" $([ "$(verdict $S verdict)/$(verdict $S step)" = fail/two ] && echo 0 || echo 1)
ok "log path in verdict exists" $([ -f "$(verdict $S log)" ] && echo 0 || echo 1)
ok "step one stamped, two not" $([ -e $S/steps/one.ok ] && [ ! -e $S/steps/two.ok ] && echo 0 || echo 1)
ok "computers file has e2e-fake-<runid>-1" $(grep -Eq '^e2e-fake-[0-9a-f]{6}-1$' $S/computers && echo 0 || echo 1)
ok "cleanup ran" $([ "$(cnt $S/cleaned)" = 1 ] && echo 0 || echo 1)
rm $S/break-two
$LIVE fake --state $S >$W/out 2>&1; rc=$?
ok "resume passes" $([ $rc = 0 ] && [ "$(verdict $S verdict)" = pass ] && echo 0 || echo 1)
ok "resume skipped step one" $([ "$(cnt $S/count-one)" = 1 ] && [ "$(cnt $S/count-two)" = 2 ] && echo 0 || echo 1)
$LIVE fake --state $S --from two >$W/out 2>&1
ok "--from two reruns two and three only" $([ "$(cnt $S/count-one)" = 1 ] && [ "$(cnt $S/count-two)" = 3 ] && [ "$(cnt $S/count-three)" = 2 ] && echo 0 || echo 1)
$LIVE fake --state $S --from nope >$W/out 2>&1; ok "--from unknown step is a usage error" $([ $? = 2 ] && echo 0 || echo 1)
$LIVE nothing --state $S >$W/out 2>&1; ok "unknown check is a usage error" $([ $? = 2 ] && echo 0 || echo 1)

S=$W/s2; mkdir -p $S; touch $S/bad-check
$LIVE fake --state $S >$W/out 2>&1; rc=$?
ok "failing check fails the step" $([ $rc = 1 ] && [ "$(verdict $S step)" = three ] && echo 0 || echo 1)

S=$W/s3; mkdir -p $S; touch $S/skip-three
$LIVE fake --state $S >$W/out 2>&1; rc=$?
ok "live_skip gives skipped, exit 3" $([ $rc = 3 ] && [ "$(verdict $S verdict)" = skipped ] && grep -q 'no key' $S/VERDICT && echo 0 || echo 1)

S=$W/s4; mkdir -p $S; touch $S/hang-two
$LIVE fake --state $S >$W/out 2>&1 & P=$!
for _ in $(seq 1 50); do [ "$(cat $S/current 2>/dev/null)" = two ] && break; sleep 0.1; done
sleep 0.5; kill -TERM $P; wait $P; rc=$?
ok "SIGTERM exits 130 with a fail verdict at two" $([ $rc = 130 ] && [ "$(verdict $S verdict)/$(verdict $S step)" = fail/two ] && grep -q interrupted $S/VERDICT && echo 0 || echo 1)
ok "interrupt ran cleanup" $([ "$(cnt $S/cleaned)" = 1 ] && echo 0 || echo 1)
rm $S/hang-two; $LIVE fake --state $S >$W/out 2>&1
ok "rerun after interrupt resumes and passes" $([ "$(verdict $S verdict)" = pass ] && [ "$(cnt $S/count-one)" = 1 ] && echo 0 || echo 1)

S=$W/s5; mkdir -p $S
LIVE_SSH=false $LIVE fake --state $S --host linux >$W/out 2>&1; rc=$?
ok "offline Linux: skipped: offline, exit 3, no step run" $([ $rc = 3 ] && grep -q 'skipped: offline' $S/VERDICT && [ ! -e $S/count-one ] && echo 0 || echo 1)
LIVE_SSH=true $LIVE fake --state $S --host linux >$W/out 2>&1
ok "online Linux runs the steps" $([ "$(verdict $S verdict)" = pass ] && [ "$(verdict $S host)" = linux ] && echo 0 || echo 1)
$LIVE fake --state $S >$W/out 2>&1
ok "switching host drops the stamps" $([ "$(cnt $S/count-one)" = 2 ] && echo 0 || echo 1)

S=$W/s6; mkdir -p $S; touch $S/hang-two
$LIVE fake --state $S >$W/out 2>&1 & P=$!
for _ in $(seq 1 50); do [ "$(cat $S/current 2>/dev/null)" = two ] && break; sleep 0.1; done
(sleep 1; kill -9 $P) & $LIVE wait $S --timeout 20 >$W/wout 2>&1; rc=$?
ok "wait reports a runner killed without verdict" $([ $rc = 1 ] && grep -q 'runner died' $W/wout && echo 0 || echo 1)
S=$W/s7; mkdir -p $S
( sleep 1; $LIVE fake --state $S >/dev/null 2>&1 ) & sleep 0.3
$LIVE wait $S --timeout 20 >$W/wout 2>&1; rc=$?
ok "wait blocks until the verdict, exits 0 on pass" $([ $rc = 0 ] && grep -q 'verdict: pass' $W/wout && echo 0 || echo 1)
$LIVE wait $W/none >/dev/null 2>&1; ok "wait on a missing dir is a usage error" $([ $? = 2 ] && echo 0 || echo 1)

# library helpers
. "$(dirname "$LIVE")/live-lib.sh"; STATE=$W/lib; mkdir -p $STATE/steps; CHECK=my_check
H=$(live_short_home); ok "short home is short and recorded" $([ ${#H} -lt 30 ] && grep -qx "$H" $STATE/tempdirs && echo 0 || echo 1)
rmdir "$H" 2>/dev/null
( live_check_sock_path "$H/.silo-dev/engine.sock" ) 2>/dev/null; ok "short socket path accepted" $?
( live_check_sock_path "$(printf 'a%.0s' $(seq 1 110))" ) >/dev/null 2>&1; ok "long socket path refused" $([ $? != 0 ] && echo 0 || echo 1)
RID=$(live_runid)
ok "computer names follow e2e-<check>-<runid>-<n>" $([ "$(live_computer)/$(live_computer)" = e2e-my-check-$RID-1/e2e-my-check-$RID-2 ] && echo 0 || echo 1)
STATE=$W/lib2; mkdir -p $STATE; ok "another run gets other computer names" $([ "$(live_computer)" != e2e-my-check-$RID-1 ] && echo 0 || echo 1)
STATE=$W/lib
mkdir $W/img; live_link $W/img $W/lnk; live_link $W/img $W/lnk; ok "live_link idempotent" $([ "$(readlink $W/lnk)" = $W/img ] && echo 0 || echo 1)
( live_link $W/nonexist $W/lnk2 ) >/dev/null 2>&1; ok "live_link refuses a missing target" $([ $? != 0 ] && echo 0 || echo 1)
( live_assert_dev_sock "$HOME/.silo/engine.sock" ) >/dev/null 2>&1; ok "production socket refused" $([ $? != 0 ] && echo 0 || echo 1)
( live_assert_dev_sock "$HOME/.silo-dev/engine.sock" ) >/dev/null 2>&1; ok "dev socket accepted" $?
# canonical paths: a fake HOME, so nothing real is looked at
FH=$W/home; mkdir -p $FH/.silo $FH/.silo-dev $W/shortbase; ln -s $FH/.silo $W/shortbase/sl-evil; ln -s $FH/.silo $FH/.silo-dev/viaink
chk() { ( HOME=$FH LIVE_SHORT_BASE=$W/shortbase live_assert_dev_sock "$1" ) >/dev/null 2>&1; }
chk "$FH/.silo-dev/../.silo/engine.sock"; ok "dotdot into ~/.silo refused" $([ $? != 0 ] && echo 0 || echo 1)
chk "$W/shortbase/sl-evil/engine.sock"; ok "symlink under sl-* into ~/.silo refused" $([ $? != 0 ] && echo 0 || echo 1)
chk "$FH/.silo-dev/viaink/engine.sock"; ok "symlink under ~/.silo-dev into ~/.silo refused" $([ $? != 0 ] && echo 0 || echo 1)
chk "$FH/elsewhere/engine.sock"; ok "path outside the Dev locations refused" $([ $? != 0 ] && echo 0 || echo 1)
chk "$FH/.silo-dev/engine.sock"; ok "plain Dev socket accepted under a fake HOME" $?
mkdir -p "$W/Silo.app/Contents/MacOS"; touch "$W/Silo.app/Contents/MacOS/silod"; ln -s "$W/Silo.app/Contents/MacOS/silod" $W/linksilod
( live_assert_dev_engine $W/linksilod ) >/dev/null 2>&1; ok "engine binary reached through a link into Silo.app refused" $([ $? != 0 ] && echo 0 || echo 1)
# engine ownership: only the process this run launched counts
FS=$W/fake-silod; cp /bin/sleep $FS 2>/dev/null
bash -c 'exec -a "$0 --agent" sleep 30' $FS & MP=$!; bash -c 'exec -a "$0 --agent" sleep 30' $FS & OP=$!
sleep 0.5; live_engine_record $MP
ok "only the recorded engine is ours" $([ "$(live_our_engine $FS)" = $MP ] && echo 0 || echo 1)
live_engine_record $$; ok "a recorded pid with another command is not ours" $([ -z "$(live_our_engine $FS)" ] && echo 0 || echo 1)
kill $MP $OP 2>/dev/null; wait $MP $OP 2>/dev/null
live_engine_record $MP; ok "a recorded pid that is gone is not ours" $([ -z "$(live_our_engine $FS)" ] && echo 0 || echo 1)
# stamps carry a fingerprint
S=$W/s8; mkdir -p $S
$LIVE fake --state $S >/dev/null 2>&1; echo "# changed" >>$LIVE_D/fake.sh; $LIVE fake --state $S >/dev/null 2>&1
ok "a changed check file drops the stamps" $([ "$(cnt $S/count-one)" = 2 ] && echo 0 || echo 1)
$LIVE fake --state $S >/dev/null 2>&1; ok "unchanged inputs keep the stamps" $([ "$(cnt $S/count-one)" = 2 ] && echo 0 || echo 1)
cat >"$LIVE_D/fp.sh" <<'C'
STEPS=(a)
step_a() { echo ran >>"$STATE/count-a"; }
fingerprint_extra() { cat "$STATE/../engine-id" 2>/dev/null; }
C
S=$W/s9; mkdir -p $S; echo one >$W/engine-id
$LIVE fp --state $S >/dev/null 2>&1; $LIVE fp --state $S >/dev/null 2>&1; echo two >$W/engine-id; $LIVE fp --state $S >/dev/null 2>&1
ok "a different engine identity drops the stamps" $([ "$(cnt $S/count-a)" = 2 ] && echo 0 || echo 1)

# cleanup that destroys the computers drops the stamps; a failing cleanup fails a pass
cat >"$LIVE_D/wipe.sh" <<'C'
STEPS=(make)
step_make() { echo ran >>"$STATE/count-make"; live_computer >/dev/null; }
cleanup() { for c in $(live_computers); do live_forget_computer "$c"; done; [ ! -e "$STATE/badclean" ]; }
C
S=$W/s10; mkdir -p $S
$LIVE wipe --state $S >/dev/null 2>&1; $LIVE wipe --state $S >/dev/null 2>&1
ok "destroyed resources drop the step stamp" $([ "$(cnt $S/count-make)" = 2 ] && [ ! -s $S/computers ] && echo 0 || echo 1)
touch $S/badclean; $LIVE wipe --state $S >/dev/null 2>&1; rc=$?
ok "cleanup failure turns pass into fail" $([ $rc = 1 ] && [ "$(verdict $S verdict)" = fail ] && grep -q 'cleanup failed' $S/VERDICT && echo 0 || echo 1)

# lock
S=$W/s11; mkdir -p $S; touch $S/hang-two
$LIVE fake --state $S >$W/out 2>&1 & P=$!
for _ in $(seq 1 50); do [ "$(cat $S/current 2>/dev/null)" = two ] && break; sleep 0.1; done
$LIVE fake --state $S >$W/out2 2>&1; rc=$?
ok "a second runner on the same state is refused" $([ $rc = 2 ] && grep -q 'already active' $W/out2 && [ ! -e $S/VERDICT ] && echo 0 || echo 1)
kill -TERM $P; wait $P 2>/dev/null; rm -f $S/hang-two
ok "the lock is released after the run" $([ ! -d $S/lock ] && echo 0 || echo 1)
mkdir -p $S/lock; echo 999999 >$S/lock/pid
$LIVE fake --state $S >/dev/null 2>&1; ok "a lock of a dead runner is taken over" $([ "$(verdict $S verdict)" = pass ] && echo 0 || echo 1)
# parallel starts: exactly one runs
S=$W/s12; mkdir -p $S; touch $S/hang-two; rm -f $W/r.*
for i in 1 2 3 4; do ( $LIVE fake --state $S >/dev/null 2>&1; echo $? >$W/r.$i ) & done
sleep 3; kill -TERM "$(cat $S/lock/pid)"; wait
ok "of four parallel runners one runs and three are refused" $([ "$(cat $W/r.* | grep -c '^2$')" = 3 ] && echo 0 || echo 1)

# wait ignores a verdict of an earlier run
S=$W/s13; mkdir -p $S/lock; sleep 30 & SP=$!
printf 'verdict: pass\nrun: old\n' >$S/VERDICT; echo new >$S/run; echo $SP >$S/lock/pid
$LIVE wait $S --timeout 2 >$W/wout 2>&1; rc=$?
ok "wait does not return a verdict of an earlier run" $([ $rc = 4 ] && echo 0 || echo 1)
( sleep 1; printf 'verdict: pass\nrun: new\n' >$S/VERDICT ) & $LIVE wait $S --timeout 8 >/dev/null 2>&1
ok "wait returns the verdict of the current run" $([ $? = 0 ] && echo 0 || echo 1)
$LIVE wait $S --timeout 2 >/dev/null 2>&1; ok "wait returns at once a verdict of the current run that already exists" $([ $? = 0 ] && echo 0 || echo 1)
$LIVE wait $S old --timeout 2 >/dev/null 2>&1; ok "wait for an explicit run id never returns another run's verdict" $([ $? = 1 ] && echo 0 || echo 1); kill $SP
# live start: returns with the run id once the runner holds the lock; wait then gets that run's verdict
cat >"$LIVE_D/instant.sh" <<'C'
STEPS=(a)
step_a() { echo "boom"; return 1; }
C
S=$W/s20; mkdir -p $S; printf 'verdict: pass\nrun: old\n' >$S/VERDICT; echo old >$S/run
out=$($LIVE start instant --state $S 2>&1); rc=$?; id=${out##* }
ok "start prints the state dir and a new run id" $([ $rc = 0 ] && [ "$out" = "$S $id" ] && [ "$id" != old ] && [ "$(cat $S/run)" = "$id" ] && echo 0 || echo 1)
$LIVE wait $S --timeout 10 >$W/wout 2>&1; rc=$?
ok "a run that fails instantly is still returned by wait" $([ $rc = 1 ] && grep -q "run: $id" $W/wout && grep -q 'verdict: fail' $W/wout && echo 0 || echo 1)
$LIVE wait $S $id --timeout 2 >/dev/null 2>&1; ok "wait with the run id returns the existing verdict at once" $([ $? = 1 ] && echo 0 || echo 1)
$LIVE wait $S old --timeout 2 >/dev/null 2>&1; ok "an old run's id is never answered with the new verdict" $([ $? = 1 ] && ! $LIVE wait $S old --timeout 2 2>/dev/null | grep -q verdict && echo 0 || echo 1)
S=$W/s21; mkdir -p $S; echo 'STEPS=(nostep)' >"$LIVE_D/broken2.sh"
out=$($LIVE start broken2 --state $S 2>/dev/null); rc=$?
$LIVE wait $S ${out##* } --timeout 5 >/dev/null 2>&1; wrc=$?
ok "start on a config error returns its run id and wait gets the fail verdict" $([ $rc = 0 ] && [ $wrc = 1 ] && [ "$(verdict $S verdict)" = fail ] && echo 0 || echo 1)
S=$W/s22; mkdir -p $S; touch $S/hang-two
$LIVE start fake --state $S >/dev/null 2>&1
$LIVE start fake --state $S >$W/out 2>&1; rc=$?
ok "start on a busy state dir fails with the runner's exit code" $([ $rc = 2 ] && grep -q 'already active' $W/out && echo 0 || echo 1)
$LIVE start nothing --state $W/s23 >/dev/null 2>&1; ok "start of an unknown check exits 2" $([ $? = 2 ] && echo 0 || echo 1)
kill -TERM "$(cat $S/lock/pid)"; $LIVE wait $S --timeout 20 >/dev/null 2>&1
# a started runner drops the old verdict before anything else
S=$W/s15; mkdir -p $S; touch $S/hang-two; printf 'verdict: pass\nrun: old\n' >$S/VERDICT
$LIVE fake --state $S >/dev/null 2>&1 & P=$!
for _ in $(seq 1 50); do [ "$(cat $S/current 2>/dev/null)" = two ] && break; sleep 0.1; done
ok "a running runner has a new run id and no old verdict" $([ ! -e $S/VERDICT ] && [ "$(cat $S/run)" != old ] && echo 0 || echo 1)
kill -TERM $P; wait $P 2>/dev/null
# a configuration error still ends in a verdict
echo 'STEPS=(nostep)' >"$LIVE_D/broken.sh"; S=$W/s16; mkdir -p $S; printf 'verdict: pass\nrun: old\n' >$S/VERDICT
$LIVE broken --state $S >/dev/null 2>&1; rc=$?
ok "an undefined step gives exit 2 and a fail verdict" $([ $rc = 2 ] && [ "$(verdict $S verdict)" = fail ] && echo 0 || echo 1)
# stale lock: concurrent takers never both run; a live takeover lock refuses
S=$W/s17; mkdir -p $S/lock; echo 999999 >$S/lock/pid; touch $S/hang-two; rm -f $W/r.*
for i in 1 2 3 4 5 6; do ( $LIVE fake --state $S >/dev/null 2>&1; echo $? >$W/r.$i ) & done
sleep 3; kill -TERM "$(cat $S/lock/pid)"; wait
ok "six runners on a stale lock: one runs, five are refused" $([ "$(cat $W/r.* | grep -c '^2$')" = 5 ] && [ "$(cnt $S/count-two)" = 1 ] && echo 0 || echo 1)
S=$W/s18; mkdir -p $S/lock $S/lock.takeover; echo 999999 >$S/lock/pid
$LIVE fake --state $S >$W/out 2>&1; rc=$?
ok "a stale lock with a takeover in progress is refused" $([ $rc = 2 ] && grep -q 'lock.takeover' $W/out && echo 0 || echo 1)
# deleting one of two computers drops the stamps
cat >"$LIVE_D/half.sh" <<'C'
STEPS=(make)
step_make() { echo ran >>"$STATE/count-make"; live_computer >/dev/null; live_computer >/dev/null; }
cleanup() { live_forget_computer "$(live_computers | head -1)"; }
C
S=$W/s19; mkdir -p $S
$LIVE half --state $S >/dev/null 2>&1; $LIVE half --state $S >/dev/null 2>&1
ok "forgetting one of two computers drops the step stamps" $([ "$(cnt $S/count-make)" = 2 ] && echo 0 || echo 1)

# interrupt stops what the step started before cleanup
cat >"$LIVE_D/kids.sh" <<'C'
STEPS=(go)
step_go() { { sleep 2; touch "$STATE/late"; } & sleep 30; }
cleanup() { [ ! -e "$STATE/late" ] && touch "$STATE/clean-before-late"; }
C
S=$W/s14; mkdir -p $S
$LIVE kids --state $S >$W/out 2>&1 & P=$!
for _ in $(seq 1 50); do [ "$(cat $S/current 2>/dev/null)" = go ] && break; sleep 0.1; done
sleep 0.5; kill -TERM $P; wait $P; sleep 3
ok "interrupt kills the step's children before cleanup" $([ ! -e $S/late ] && [ -e $S/clean-before-late ] && echo 0 || echo 1)

# machine-wide lock per shared system
cat >"$LIVE_D/locked.sh" <<'C'
CHECK_LOCKS="dev-engine"
STEPS=(a)
step_a() { echo ran >>"$STATE/count-a"; [ -e "$STATE/hang" ] && { sleep 30 & echo $! >"$STATE/sleep.pid"; wait; }; return 0; }
C
S=$W/s30; mkdir -p $S; touch $S/hang
$LIVE locked --state $S >$W/out 2>&1 & P=$!
for _ in $(seq 1 50); do [ "$(cat $S/current 2>/dev/null)" = a ] && break; sleep 0.1; done
ok "the lock file names the holder" $(grep -q "pid $P," $W/locks/dev-engine.lock && grep -q "check locked" $W/locks/dev-engine.lock && echo 0 || echo 1)
S2=$W/s31; mkdir -p $S2
$LIVE locked --state $S2 >$W/out2 2>&1; rc=$?
ok "a second check on the same shared system is refused: exit 3 naming the holder" $([ $rc = 3 ] && grep -q "busy: dev-engine is held" $W/out2 && grep -q "pid $P," $W/out2 && [ ! -e $S2/run ] && echo 0 || echo 1)
$LIVE fake --state $W/s32 >/dev/null 2>&1; ok "a check without CHECK_LOCKS is not blocked" $([ "$(verdict $W/s32 verdict)" = pass ] && echo 0 || echo 1)
$LIVE start locked --state $S2 >$W/out2 2>&1; rc=$?
ok "start of a refused check exits 3 with the reason" $([ $rc = 3 ] && grep -q 'busy: dev-engine' $W/out2 && echo 0 || echo 1)
kill -TERM $P; wait $P 2>/dev/null; rm -f $S/hang
$LIVE locked --state $S2 >$W/out2 2>&1; rc=$?
ok "the lock is released when the holder ends" $([ $rc = 0 ] && [ "$(verdict $S2 verdict)" = pass ] && echo 0 || echo 1)
S=$W/s33; mkdir -p $S; touch $S/hang
$LIVE locked --state $S >/dev/null 2>&1 & P=$!
for _ in $(seq 1 50); do [ "$(cat $S/current 2>/dev/null)" = a ] && break; sleep 0.1; done
for _ in $(seq 1 50); do [ -s $S/sleep.pid ] && break; sleep 0.1; done
kill -KILL "$P"; wait $P 2>/dev/null; sleep 0.5
$LIVE locked --state $W/s34 >/dev/null 2>&1; rc=$?
ok "a killed holder's lock stays while something it started still runs" $([ $rc = 3 ] && echo 0 || echo 1)
kill -TERM "$(cat $S/sleep.pid)" 2>/dev/null; sleep 0.5
$LIVE locked --state $W/s34 >/dev/null 2>&1; ok "the lock is released once the killed holder and what it started are gone" $([ "$(verdict $W/s34 verdict)" = pass ] && echo 0 || echo 1)
LIVE_LOCK_DIR=$W/locks2 $LIVE locked --state $W/s35 >/dev/null 2>&1; ok "LIVE_LOCK_DIR moves the lock folder" $([ -f $W/locks2/dev-engine.lock ] && echo 0 || echo 1)
$LIVE locked --state $W/s36 >/dev/null 2>&1 & $LIVE locked --state $W/s37 >/dev/null 2>&1 & $LIVE locked --state $W/s38 >/dev/null 2>&1 & wait
ok "parallel runs of a locked check all end with a verdict or a refusal" $(for d in s36 s37 s38; do [ "$(verdict $W/$d verdict 2>/dev/null)" = pass ] || [ ! -e $W/$d/run ]; done && echo 0 || echo 1)

# own engine instance: a check with CHECK_OWN_ENGINE runs beside the lock holder, each run with its own HOME
cat >"$LIVE_D/own.sh" <<'C'
CHECK_LOCKS="dev-engine"
CHECK_OWN_ENGINE="dev-engine"
STEPS=(a)
step_a() {
  echo "$LIVE_ENGINE" >"$STATE/engine"; live_dev_sock >"$STATE/sock"; live_engine_home >"$STATE/home"; live_engine_prepare
  live_engine_launch "$STATE/../fake-silod" >"$STATE/launch.out" 2>&1 & wait $!
}
C
printf '#!/bin/sh\necho "HOME=$HOME PORT=$SILO_URL_PROXY_PORT ARGS=$*"\n' >$W/fake-silod; chmod +x $W/fake-silod
mkdir -p "$FH/Library/Application Support/org.silo.dev/guest-image/img"; echo base >"$FH/Library/Application Support/org.silo.dev/guest-image/img/disk"; chmod -R a-w "$FH/Library/Application Support/org.silo.dev/guest-image/img"
SB=$(mktemp -d /tmp/lt.XXXX); trap 'chmod -R u+w "$W" "$SB"; rm -rf "$W" "$SB"' EXIT  # engine homes must be short (LIVE_ENGINE_HOME_MAX)
S=$W/s50; mkdir -p $S; touch $S/hang
$LIVE locked --state $S >/dev/null 2>&1 & P=$!
for _ in $(seq 1 50); do [ "$(cat $S/current 2>/dev/null)" = a ] && break; sleep 0.1; done
for i in 1 2; do mkdir -p $W/o$i; cp $W/fake-silod $W/o$i/; done
HOME=$FH LIVE_SHORT_BASE=$SB $LIVE own --state $W/o1/s >/dev/null 2>&1 & A=$!
HOME=$FH LIVE_SHORT_BASE=$SB $LIVE own --state $W/o2/s >/dev/null 2>&1 & B=$!; wait $A $B
ok "two own-engine runs pass while the dev-engine lock is held" $([ "$(verdict $W/o1/s verdict)/$(verdict $W/o2/s verdict)" = pass/pass ] && echo 0 || echo 1)
ok "an own-engine run sees LIVE_ENGINE=own" $([ "$(cat $W/o1/s/engine)" = own ] && echo 0 || echo 1)
H1=$(cat $W/o1/s/home); H2=$(cat $W/o2/s/home)
ok "each run has its own engine HOME under the short base" $([ "$H1" != "$H2" ] && [ "${H1#$SB/sl-}" != "$H1" ] && [ "$(cat $W/o1/s/sock)" = "$H1/.silo-dev/engine.sock" ] && echo 0 || echo 1)
ok "the engine starts with its HOME and an ephemeral proxy port" $([ "$(cat $W/o1/s/launch.out)" = "HOME=$H1 PORT=0 ARGS=--agent" ] && echo 0 || echo 1)
G1="$H1/Library/Application Support/org.silo.dev/guest-image/img/disk"
chmod u+w "$G1" "$(dirname "$G1")"; echo changed >"$G1"
ok "guest images are a copy: the instance's writes never reach the original" $([ "$(cat "$FH/Library/Application Support/org.silo.dev/guest-image/img/disk")" = base ] && echo 0 || echo 1)
( HOME=$FH LIVE_SHORT_BASE=$SB bash -c ". $LIVE_LIB_DIR/live-lib.sh; STATE=$W/o1/s; live_assert_dev_sock \"\$(cat $W/o1/s/sock)\"" ) >/dev/null 2>&1
ok "the own engine's socket counts as a Dev path" $?
HOME=$FH LIVE_SHORT_BASE=$SB $LIVE own --state $W/o3 --engine shared >$W/out 2>&1; rc=$?
ok "--engine shared takes the lock again: exit 3 naming the holder" $([ $rc = 3 ] && grep -q "busy: dev-engine is held" $W/out && echo 0 || echo 1)
HOME=$FH LIVE_ENGINE=shared LIVE_SHORT_BASE=$SB $LIVE own --state $W/o3 >/dev/null 2>&1
ok "LIVE_ENGINE=shared also takes the lock" $([ $? = 3 ] && echo 0 || echo 1)
$LIVE own --state $W/o3 --engine other >/dev/null 2>&1; ok "--engine with another value is a usage error" $([ $? = 2 ] && echo 0 || echo 1)
kill -TERM $P; wait $P 2>/dev/null
HOME=$FH LIVE_SHORT_BASE=$SB $LIVE own --state $W/o3 --engine shared >/dev/null 2>&1
ok "a shared run uses the real HOME and the default socket" $([ "$(cat $W/o3/engine)" = shared ] && [ "$(cat $W/o3/home)" = "$FH" ] && [ "$(cat $W/o3/sock)" = "$FH/.silo-dev/engine.sock" ] && echo 0 || echo 1)
( HOME=$FH LIVE_SHORT_BASE=$SB LIVE_ENGINE=own bash -c ". $LIVE_LIB_DIR/live-lib.sh; STATE=$W/o1/s; live_engine_remove" ) >/dev/null 2>&1
ok "live_engine_remove deletes the own engine home with its read-only guest images" $([ $? = 0 ] && [ ! -e "$H1" ] && [ ! -e $W/o1/s/engine-home ] && echo 0 || echo 1)
own_prep() { ( HOME=$1 LIVE_SHORT_BASE=$2 LIVE_ENGINE=own bash -c ". $LIVE_LIB_DIR/live-lib.sh; STATE=\$1; mkdir -p \$STATE; live_engine_prepare" _ "$3" ) >/dev/null 2>&1; }
own_prep "$FH" "$SB" $W/o4; ok "a prepared own engine home is accepted" $?
mkdir -p "$SB/in/Library/Application Support/org.silo.dev/guest-image"
own_prep "$SB/in" "$SB/in" $W/o5; ok "an own engine HOME inside the real HOME is refused" $([ $? != 0 ] && echo 0 || echo 1)
own_prep "$FH" "$W/shortbase" $W/o6; ok "an own engine HOME longer than silod accepts is refused" $([ $? != 0 ] && echo 0 || echo 1)

# a killed run leaves its own engine home with read-only guest images: cleanup and `run sweep` remove it
AK=$W/atk; mkdir -p $AK
own_left() { ( HOME=$FH LIVE_SHORT_BASE=$SB ATELIER_TMP=$AK LIVE_ENGINE=own CHECK=own bash -c ". $LIVE_LIB_DIR/live-lib.sh; STATE=\$1; mkdir -p \$STATE; echo own >\$STATE/check; live_engine_prepare; live_engine_home" _ "$1" ) 2>/dev/null; }
K1=$(own_left $W/k1)
( rm -rf "$K1" ) 2>/dev/null; ok "a killed run's engine home resists plain rm -rf (read-only guest images)" $([ -e "$K1/Library/Application Support/org.silo.dev/guest-image/img/disk" ] && echo 0 || echo 1)
ok "the engine home is a leftovers line while it exists" $(grep -qxF "engine home $K1 (live check state $(live_canon $W/k1); remove with: run sweep $(live_canon $W/k1))" $AK/leftovers && echo 0 || echo 1)
cat >"$LIVE_D/ownclean.sh" <<'C'
CHECK_OWN_ENGINE="dev-engine"
STEPS=(a)
step_a() { :; }
cleanup() { live_engine_remove; }
C
HOME=$FH LIVE_SHORT_BASE=$SB ATELIER_TMP=$AK $LIVE ownclean --state $W/k1 >/dev/null 2>&1
ok "the next run's cleanup removes the left engine home and its leftovers line" $([ $? = 0 ] && [ ! -e "$K1" ] && [ ! -e $W/k1/engine-home ] && ! grep -q "$K1" $AK/leftovers && echo 0 || echo 1)
K2=$(own_left $W/k2); K3=$(own_left $W/k3); K4=$(own_left $W/k4); K5=$(own_left $W/k5)
echo e2e-own-x-1 >$W/k3/computers
mkdir -p $W/k4/lock; echo $$ >$W/k4/lock/pid
mkdir -p "$K5/.silo-dev"; python3 -I -c 'import socket,sys,time
s=socket.socket(socket.AF_UNIX); s.bind(sys.argv[1]); s.listen(); open(sys.argv[2],"w").close(); time.sleep(30)' "$K5/.silo-dev/engine.sock" $W/k5/up & SP=$!
for _ in $(seq 1 50); do [ -e $W/k5/up ] && break; sleep 0.1; done
HOME=$FH LIVE_SHORT_BASE=$SB ATELIER_TMP=$AK $LIVE sweep $W/k2 $W/k3 $W/k4 $W/k5 >$W/out 2>&1; rc=$?; cp $W/out $W/out0
kill $SP; wait $SP 2>/dev/null
ok "sweep removes an idle state dir's read-only engine home and its leftovers line" $([ ! -e "$K2" ] && [ ! -e $W/k2/engine-home ] && ! grep -q "$K2" $AK/leftovers && grep -qxF "removed $K2" $W/out && echo 0 || echo 1)
ok "sweep keeps a home whose state still lists computers" $([ -d "$K3" ] && grep -q "kept $K3: .* still lists computers" $W/out && echo 0 || echo 1)
ok "sweep keeps a home whose state dir has an active run" $([ -d "$K4" ] && grep -q "kept $K4: a run is active" $W/out && [ "$(cat $W/k4/lock/pid)" = $$ ] && echo 0 || echo 1)
ok "sweep keeps a home whose engine still answers" $([ -d "$K5" ] && grep -q "kept $K5: its engine still answers" $W/out && echo 0 || echo 1)
ok "sweep exits 1 when it kept a home, and releases the state locks it took" $([ $rc = 1 ] && [ ! -e $W/k2/lock ] && [ ! -e $W/k5/lock ] && echo 0 || echo 1)
rm -rf $W/k4/lock $W/k3/computers; rm -f "$K5/.silo-dev/engine.sock"
HOME=$FH LIVE_SHORT_BASE=$SB ATELIER_TMP=$AK $LIVE sweep $W/k3 $W/k4 $W/k5 >$W/out 2>&1
ok "sweep then removes them all; leftovers is empty" $([ $? = 0 ] && [ ! -e "$K3" ] && [ ! -e "$K4" ] && [ ! -e "$K5" ] && [ ! -s $AK/leftovers ] && echo 0 || echo 1)
mkdir -p $AK/live; ln -s "$W/k1" $AK/live/k1; K6=$(own_left $AK/live/k6)
HOME=$FH LIVE_SHORT_BASE=$SB ATELIER_TMP=$AK $LIVE sweep >$W/out 2>&1
ok "sweep without arguments covers every state dir under the default base" $([ $? = 0 ] && [ ! -e "$K6" ] && echo 0 || echo 1)
ok "sweep keeps a home whose engine answers and says how to stop it" $(grep -q "kept $K5: its engine still answers.*; stop .*then rerun: run sweep" $W/out0 && echo 0 || echo 1)
# an engine that is alive before its socket answers keeps its home
K7=$(own_left $W/k7); mkdir -p $W/k7bin; cp /bin/sleep $W/k7bin/silod
bash -c 'exec -a "$0 --agent" sleep 30' $W/k7bin/silod & EP=$!; sleep 0.3; echo $EP >$W/k7/engine.pid
K8=$(own_left $W/k8); mkdir -p "$K8/.silo-dev"
python3 -I -c 'import fcntl,os,sys,time
f=os.open(sys.argv[1],os.O_RDWR|os.O_CREAT,0o600); fcntl.flock(f,fcntl.LOCK_EX); open(sys.argv[2],"w").close(); time.sleep(30)' "$K8/.silo-dev/engine.lock" $W/k8/up & LP=$!
for _ in $(seq 1 50); do [ -e $W/k8/up ] && break; sleep 0.1; done
HOME=$FH LIVE_SHORT_BASE=$SB ATELIER_TMP=$AK $LIVE sweep $W/k7 $W/k8 >$W/out 2>&1; rc=$?
kill $EP $LP 2>/dev/null; wait $EP $LP 2>/dev/null
ok "sweep keeps a home whose recorded engine still runs before its socket answers, naming the stop command" $([ $rc = 1 ] && [ -d "$K7" ] && grep -q "kept $K7: .*kill -TERM $EP" $W/out && echo 0 || echo 1)
ok "sweep keeps a home whose engine lock is held while its socket is silent" $([ -d "$K8" ] && grep -q "kept $K8: .*engine.lock" $W/out && echo 0 || echo 1)
HOME=$FH LIVE_SHORT_BASE=$SB ATELIER_TMP=$AK $LIVE sweep $W/k7 $W/k8 >$W/out 2>&1
ok "sweep removes them once the engine and the lock are gone" $([ $? = 0 ] && [ ! -e "$K7" ] && [ ! -e "$K8" ] && echo 0 || echo 1)
# an engine home is removed only as itself, directly under the short base
mkdir -p $W/victim; echo keep >$W/victim/f; ln -s $W/victim $SB/sl-evl
mkdir -p $W/k9; echo own >$W/k9/check; echo "$SB/sl-evl/" >$W/k9/engine-home
HOME=$FH LIVE_SHORT_BASE=$SB $LIVE sweep $W/k9 >$W/out 2>&1; rc=$?
ok "sweep never removes through an engine home recorded as a symlink with a trailing slash" $([ $rc = 1 ] && [ -f $W/victim/f ] && [ -L $SB/sl-evl ] && echo 0 || echo 1)
mkdir -p $W/other/sl-abc $W/k11; echo own >$W/k11/check; echo "$W/other/sl-abc" >$W/k11/engine-home
HOME=$FH LIVE_SHORT_BASE=$SB $LIVE sweep $W/k11 >$W/out 2>&1; rc=$?
ok "sweep refuses an engine home that is not directly under the short base" $([ $rc = 1 ] && [ -d $W/other/sl-abc ] && echo 0 || echo 1)
mkdir -p $W/rel/rb
( cd $W/rel && own_prep "$FH" rb $W/o7 ); rc1=$?
( cd $W/rel && ( HOME=$FH ATELIER_TMP=rb LIVE_ENGINE=own bash -c ". $LIVE_LIB_DIR/live-lib.sh; STATE=\$1; mkdir -p \$STATE; live_engine_home" _ $W/o8 ) >/dev/null 2>&1 ); rc2=$?
ok "a relative short base (LIVE_SHORT_BASE or ATELIER_TMP) is refused" $([ $rc1 != 0 ] && [ $rc2 != 0 ] && [ -z "$(ls -A $W/rel/rb)" ] && echo 0 || echo 1)
# the leftovers line is the same whichever path names the state dir
K10=$(own_left $W/k10); ln -s $W/k10 $W/k10link
HOME=$FH LIVE_SHORT_BASE=$SB ATELIER_TMP=$AK $LIVE sweep $W/k10link >$W/out 2>&1
ok "sweep through a link to the state dir drops the home's leftovers line" $([ ! -e "$K10" ] && ! grep -q "$K10" $AK/leftovers && echo 0 || echo 1)
# sweep without arguments: every TMPDIR layout and the ATELIER_TMP one
TD=$W/tmpd; K12=$(own_left $TD/silo-agent/item1/live/c1); K13=$(own_left $TD/silo-agent/live/c2); K14=$(own_left $AK/live/c3)
TMPDIR=$TD HOME=$FH LIVE_SHORT_BASE=$SB ATELIER_TMP=$AK $LIVE sweep >$W/out 2>&1
ok "sweep without arguments covers TMPDIR/silo-agent/[*/]live/* and ATELIER_TMP/live/*" $([ $? = 0 ] && [ ! -e "$K12" ] && [ ! -e "$K13" ] && [ ! -e "$K14" ] && echo 0 || echo 1)

# the engine mode a state dir was used with holds while it lists computers or an engine home
cat >"$LIVE_D/mode.sh" <<'C'
CHECK_LOCKS="dev-engine"
CHECK_OWN_ENGINE="dev-engine"
STEPS=(a)
step_a() { live_engine_home >/dev/null; live_computer >/dev/null; }
cleanup() { echo "$LIVE_ENGINE" >>"$STATE/cleaned"; [ -e "$STATE/keep" ] && return 1; for c in $(live_computers); do live_forget_computer "$c"; done; live_engine_remove; }
C
S=$W/m1; mkdir -p $S; touch $S/keep
HOME=$FH LIVE_SHORT_BASE=$SB $LIVE mode --state $S --engine shared >/dev/null 2>&1
HOME=$FH LIVE_SHORT_BASE=$SB $LIVE mode --state $S >$W/out 2>&1; rc=$?
ok "an own run on a state dir a shared run left computers in is refused before its cleanup" $([ $rc = 2 ] && grep -q 'engine shared' $W/out && [ "$(cnt $S/cleaned)" = 1 ] && [ ! -e $S/engine-home ] && [ -s $S/computers ] && echo 0 || echo 1)
rm $S/keep; HOME=$FH LIVE_SHORT_BASE=$SB $LIVE mode --state $S --engine shared >/dev/null 2>&1
HOME=$FH LIVE_SHORT_BASE=$SB $LIVE mode --state $S >/dev/null 2>&1
ok "once that run's cleanup is done, the other mode is allowed" $([ "$(verdict $S verdict)" = pass ] && [ "$(tail -1 $S/cleaned)" = own ] && echo 0 || echo 1)
S=$W/m2; mkdir -p $S; touch $S/keep
HOME=$FH LIVE_SHORT_BASE=$SB $LIVE mode --state $S >/dev/null 2>&1; MH=$(cat $S/engine-home)
HOME=$FH LIVE_SHORT_BASE=$SB $LIVE mode --state $S --engine shared >$W/out 2>&1; rc=$?
ok "a shared run on a state dir an own run left a home in is refused before its cleanup" $([ $rc = 2 ] && grep -q 'engine own' $W/out && [ "$(cnt $S/cleaned)" = 1 ] && [ -d "$MH" ] && echo 0 || echo 1)
rm -f $S/engine-mode $S/computers
HOME=$FH LIVE_SHORT_BASE=$SB $LIVE mode --state $S --engine shared >/dev/null 2>&1
ok "a state dir from before the mode record counts as own while it has an engine home" $([ $? = 2 ] && [ -d "$MH" ] && echo 0 || echo 1)
rm $S/keep; HOME=$FH LIVE_SHORT_BASE=$SB $LIVE mode --state $S >/dev/null 2>&1
ok "the own run cleans up its home" $([ "$(verdict $S verdict)" = pass ] && [ ! -e "$MH" ] && echo 0 || echo 1)

# under atelier tmp: state, short home and leftovers live in $ATELIER_TMP
cat >"$LIVE_D/vm.sh" <<'C'
STEPS=(make)
step_make() { echo ran >>"$STATE/count-make"; live_computer_named a >/dev/null; live_computer >/dev/null; cp "$ATELIER_TMP/leftovers" "$STATE/leftovers-mid"; }
cleanup() { [ -e "$STATE/keep" ] && return 1; for c in $(live_computers); do live_forget_computer "$c"; done; }
C
AT=$W/atl; mkdir -p $AT
ATELIER_TMP=$AT $LIVE vm >$W/out 2>&1; rc=$?
ok "ATELIER_TMP holds the default state dir" $([ $rc = 0 ] && [ "$(verdict $AT/live/vm verdict)" = pass ] && echo 0 || echo 1)
ok "each computer is a leftovers line while it exists" $([ "$(grep -c '^computer e2e-vm-' $AT/live/vm/leftovers-mid)" = 2 ] && echo 0 || echo 1)
ok "leftovers is empty once the computers are gone" $([ -f $AT/leftovers ] && [ ! -s $AT/leftovers ] && echo 0 || echo 1)
mkdir -p $AT/live/vm; touch $AT/live/vm/keep; ATELIER_TMP=$AT $LIVE vm --from make >$W/out 2>&1; rc=$?
ok "a computer that is not gone stays in leftovers" $([ $rc = 1 ] && [ "$(grep -c '^computer e2e-vm-' $AT/leftovers)" = 2 ] && echo 0 || echo 1)
rm -f $AT/leftovers; mkdir $W/atl2; rm $AT/live/vm/keep
ATELIER_TMP=$W/atl2 $LIVE vm --state $AT/live/vm >$W/out 2>&1
ok "computers a resumed state still lists join the new folder's leftovers, then go" $([ ! -s $W/atl2/leftovers ] && [ -f $W/atl2/leftovers ] && echo 0 || echo 1)
H=$(ATELIER_TMP=$AT bash -c ". $LIVE_LIB_DIR/live-lib.sh; STATE=$W/h; mkdir -p \$STATE; live_short_home" 2>/dev/null)
ok "short home goes under ATELIER_TMP" $([ "${H#$AT/sl-}" != "$H" ] && echo 0 || echo 1)
( ATELIER_TMP=$AT HOME=$FH bash -c ". $LIVE_LIB_DIR/live-lib.sh; STATE=$W/h; mkdir -p \$STATE; live_assert_dev_sock $AT/sl-1/engine.sock" ) >/dev/null 2>&1
ok "a socket under the ATELIER_TMP short home counts as a Dev path" $?
echo "$N tests, $BAD failed"; [ $BAD = 0 ]
