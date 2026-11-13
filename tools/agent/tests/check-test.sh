#!/usr/bin/env bash
# Tests tools/agent/check: runtime restore from runtime-cache, flaky log outside the repo, and fmt that
# flags only formatting the change introduced. Temporary git repos and stubbed cargo; rustfmt is real.
set -uo pipefail
unset BOARD BOARD_INTEGRATION SILO_INTEGRATION_BRANCH SILO_AGENT_ITEM SILO_AGENT_CACHE RUNTIME_PREPARE_CMD SILO_BIN SILO_RUNTIME_TARGET SILO_BUILD_STATE LIVE_D
A=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
SRC=$(cd "$A/../.." && pwd)
W=$(mktemp -d "${TMPDIR:-/tmp}/check-test.XXXXXX"); W=$(cd "$W" && pwd -P); trap 'rm -rf "$W"' EXIT
export TMPDIR=$W/tmp; mkdir -p "$TMPDIR"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
N=0; BAD=0
ok() { N=$((N+1)); if [ "$2" = 0 ]; then echo "PASS $1"; else echo "FAIL $1 ${3:-}"; BAD=$((BAD+1)); fi; }
t() { if eval "$2"; then ok "$1" 0; else ok "$1" 1 "${3:-}"; fi; }

# A repository with one crate (silo-ui) and the two tools under test.
R=$W/main; mkdir -p $R/tools/agent $R/app/SiloUI/scripts $R/app/SiloUI/.config $R/app/SiloUI/src-tauri/src
git -C $R init -q -b main
cp $A/check $A/affected $R/tools/agent/; cp $SRC/app/SiloUI/scripts/ci-native-changes.py $R/app/SiloUI/scripts/
cp $SRC/rust-toolchain.toml $R/
printf '[profile.check]\nretries = 1\n[profile.check.junit]\npath = "junit.xml"\n' >$R/app/SiloUI/.config/nextest.toml
cat >$R/tools/agent/runtime-cache <<'S'
#!/bin/sh
d=$(cd "$(dirname "$0")/../.." && pwd)
if [ "$1" = --status ]; then echo "key k"; echo "${RC_STATUS:-hit}"; exit 0; fi
echo "$*" >>"$RC_LOG"
mkdir -p "$d/app/SiloUI/src-tauri/binaries" && touch "$d/app/SiloUI/src-tauri/binaries/msb-x"
S
chmod +x $R/tools/agent/runtime-cache
printf 'fn  old( ) {}\n\nfn kept() {}\n' >$R/app/SiloUI/src-tauri/src/lib.rs   # not rustfmt-clean at the base
git -C $R add -A; git -C $R commit -qm init
mkdir -p $W/bin
cat >$W/bin/cargo <<'S'
#!/bin/sh
case "$1" in
  metadata) printf '{"packages":[{"id":"1","name":"silo-ui","edition":"2021","manifest_path":"%s/app/SiloUI/src-tauri/Cargo.toml","dependencies":[]}]}' "$ROOT_UNDER_TEST";;
  nextest) case "$2" in
      --version) exit 1;;
    esac;;
  test) [ -n "$FLAKY_JUNIT" ] && exit 0;;
esac
exit 0
S
chmod +x $W/bin/cargo
export PATH=$W/bin:$PATH RC_LOG=$W/rc.log
touch $R/app/SiloUI/src-tauri/Cargo.toml
printf 'target/\nbinaries/\n__pycache__/\n' >$R/.gitignore
git -C $R add -A; git -C $R commit -qm crate -q
# the integration checkout is $R (branch main); all work happens in a linked worktree
WT=$W/wt; git -C $R worktree add -q $WT -b work
export ROOT_UNDER_TEST=$WT
CK() { (cd $WT && python3 -I tools/agent/check "$@") 2>&1; }

# --- runtime: restored from the cache on a hit, never prepared without --prepare
echo '// c' >>$WT/app/SiloUI/src-tauri/src/lib.rs
out=$(RC_STATUS=hit CK --quick); rc=$?
t "a runtime-cache hit restores the runtime before the checks" '[ -f $WT/app/SiloUI/src-tauri/binaries/msb-x ] && grep -q "restored the prepared runtime" <<<"$out" && ! grep -q "NOT checked" <<<"$out"' "$out"
rm -rf $WT/app/SiloUI/src-tauri/binaries; : >$W/rc.log
out=$(RC_STATUS=miss CK --quick); rc=$?
t "a miss without --prepare does not start a prepare and reports PARTIAL" '[ $rc = 2 ] && [ ! -s $W/rc.log ] && grep -q "NOT checked or tested" <<<"$out" && grep -q PARTIAL <<<"$out"' "$out"
out=$(RC_STATUS=miss CK --quick --prepare); rc=$?
t "--prepare runs runtime-cache on a miss" '[ -s $W/rc.log ] && [ -f $WT/app/SiloUI/src-tauri/binaries/msb-x ]' "$out"

# --- fmt: pre-existing unformatted code is a note; formatting the change introduced is RED
git -C $WT reset -q --hard
printf 'fn  old( ) {}\n\nfn kept() {}\n// a comment\n' >$WT/app/SiloUI/src-tauri/src/lib.rs
out=$(CK --quick); rc=$?
t "a change next to pre-existing unformatted code is not RED" '[ $rc != 1 ] && grep -q "pre-existing" <<<"$out" && ! grep -q "fmt FAILED" <<<"$out"' "$out"
printf 'fn  old( ) {}\n\nfn kept() {}\nfn   added( ) {}\n' >$WT/app/SiloUI/src-tauri/src/lib.rs
out=$(CK --quick); rc=$?
t "unformatted code the change adds is RED" '[ $rc = 1 ] && grep -q "fmt FAILED" <<<"$out" && grep -q "lib.rs:4" <<<"$out"' "$out"
printf 'fn  old( ) {}\n\nfn kept() {}\nfn added() {}\n' >$WT/app/SiloUI/src-tauri/src/lib.rs
out=$(CK --quick); rc=$?
t "formatted additions are not RED" '! grep -q "fmt FAILED" <<<"$out"' "$out"
git -C $WT reset -q --hard
printf 'fn  old( ) {}\n\nfn   kept ( ) {}\n' >$WT/app/SiloUI/src-tauri/src/lib.rs
out=$(CK --quick); rc=$?
t "editing a line that rustfmt would change is RED" '[ $rc = 1 ] && grep -q "fmt FAILED" <<<"$out"' "$out"
git -C $WT reset -q --hard
printf 'fn  new( ) {}\n' >$WT/app/SiloUI/src-tauri/src/new.rs
out=$(CK --quick); rc=$?
t "a new unformatted file is RED" '[ $rc = 1 ] && grep -q "new.rs" <<<"$out"' "$out"
rm $WT/app/SiloUI/src-tauri/src/new.rs

# --- flaky tests go to flaky.log in the state directory, never into the repo
git -C $WT reset -q --hard
echo '// c' >>$WT/app/SiloUI/src-tauri/src/lib.rs
cat >$W/bin/cargo <<'S'
#!/bin/sh
case "$1" in
  metadata) printf '{"packages":[{"id":"1","name":"silo-ui","edition":"2021","manifest_path":"%s/app/SiloUI/src-tauri/Cargo.toml","dependencies":[]}]}' "$ROOT_UNDER_TEST";;
  nextest)
    [ "$2" = --version ] && { echo "cargo-nextest 0.9.151"; exit 0; }
    cfg=$4; junit=$(sed -n 's/^path = "\(.*\)"/\1/p' "$cfg")
    printf '<testsuites><testsuite><testcase classname="silo-ui" name="a::flaky_one"><flakyFailure/></testcase><testcase classname="silo-ui" name="a::fine"/></testsuite></testsuites>' >"$junit";;
esac
exit 0
S
mkdir -p $WT/app/SiloUI/src-tauri/binaries; touch $WT/app/SiloUI/src-tauri/binaries/msb-x
out=$(CK); rc=$?
S=$R.state
t "flaky tests are recorded in <integration checkout>.state/flaky.log" 'grep -q "silo-ui a::flaky_one. failed, then passed on retry" $S/flaky.log && ! grep -q fine $S/flaky.log' "$out"
t "no tracked file is touched by the flaky record" '[ ! -e $WT/docs/workflow/FLAKY.md ] && [ -z "$(git -C $WT status --porcelain | grep -v "^ M app/SiloUI/src-tauri/src/lib.rs")" ]' "$(git -C $WT status --porcelain)"
t "the flaky note names the log" 'grep -q "flaky.log" <<<"$out"' "$out"
rm -rf $WT/app/SiloUI/target/agent-check/green
out=$(CK); n=$(grep -c flaky_one $S/flaky.log)
t "a test already in the log is not added again" '[ "$n" = 1 ]' "$n"
rm -rf $S
out=$(SILO_BUILD_STATE=$W/custom CK); rm -rf $WT/app/SiloUI/target/agent-check/green
out=$(SILO_BUILD_STATE=$W/custom CK)
t "SILO_BUILD_STATE overrides the location" '[ -s $W/custom/flaky.log ]' "$out"
# no integration checkout: a temp folder
rm -rf $WT/app/SiloUI/target/agent-check/green
out=$(SILO_INTEGRATION_BRANCH=gone CK --base main)
t "without an integration checkout the log goes to \$TMPDIR/silo-agent" '[ -s $TMPDIR/silo-agent/flaky.log ]' "$out"

echo "$N tests, $BAD failed"; [ $BAD = 0 ]
