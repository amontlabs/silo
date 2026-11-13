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
# the old command still reaches the runner
OLD=$(cd "$(dirname "$LIVE")/../../../.." && pwd)/tools/agent/live
LIVE_D=$LIVE_D "$OLD" fake --state $W/s40 >/dev/null 2>&1; ok "tools/agent/live execs the new runner" $([ "$(verdict $W/s40 verdict)" = pass ] && echo 0 || echo 1)
echo "$N tests, $BAD failed"; [ $BAD = 0 ]
