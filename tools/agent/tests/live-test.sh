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
ok "computers file has e2e-fake-1" $([ "$(cat $S/computers)" = e2e-fake-1 ] && echo 0 || echo 1)
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
ok "computer names follow e2e-<check>-<n>" $([ "$(live_computer)/$(live_computer)" = e2e-my-check-1/e2e-my-check-2 ] && echo 0 || echo 1)
mkdir $W/img; live_link $W/img $W/lnk; live_link $W/img $W/lnk; ok "live_link idempotent" $([ "$(readlink $W/lnk)" = $W/img ] && echo 0 || echo 1)
( live_link $W/nonexist $W/lnk2 ) >/dev/null 2>&1; ok "live_link refuses a missing target" $([ $? != 0 ] && echo 0 || echo 1)
( live_assert_dev_sock "$HOME/.silo/engine.sock" ) >/dev/null 2>&1; ok "production socket refused" $([ $? != 0 ] && echo 0 || echo 1)
( live_assert_dev_sock "$HOME/.silo-dev/engine.sock" ) >/dev/null 2>&1; ok "dev socket accepted" $?
echo "$N tests, $BAD failed"; [ $BAD = 0 ]
