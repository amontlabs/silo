#!/usr/bin/env bash
# Tests the live runner with a fake check: pass, fail, resume, --from, interrupt, offline Linux,
# skip, failing checks, wait on a dead runner, short paths. Touches no engine.
set -uo pipefail
LIVE=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/live
W=$(mktemp -d "${TMPDIR:-/tmp}/live-test.XXXXXX"); trap 'rm -rf "$W"' EXIT
export LIVE_D=$W/d; mkdir -p "$LIVE_D"
N=0; BAD=0
ok() { N=$((N+1)); if [ "$2" = 0 ]; then echo "PASS $1"; else echo "FAIL $1 ${3:-}"; BAD=$((BAD+1)); fi; }
verdict() { sed -n "s/^$2: //p" "$1/VERDICT"; }
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
$LIVE wait $S --timeout 2 >/dev/null 2>&1; ok "wait ignores a verdict that was already there when it began, even with the current run id" $([ $? = 4 ] && echo 0 || echo 1); kill $SP
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
echo "$N tests, $BAD failed"; [ $BAD = 0 ]
