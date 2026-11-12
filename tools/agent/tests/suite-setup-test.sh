#!/usr/bin/env bash
# Tests the full-suite script of merge-batch (native runtime, build file, evals and pytest handling)
# and merge-batch --last (failure recognition). Temporary git repos and stubbed tools only.
set -uo pipefail
A=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
W=$(mktemp -d "${TMPDIR:-/tmp}/suite-setup-test.XXXXXX"); W=$(cd "$W" && pwd -P); trap 'rm -rf "$W"' EXIT
export TMPDIR=$W/tmp; mkdir -p "$TMPDIR"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
export SILO_AGENT_CACHE=$W/cache
unset SILO_GITHUB_APP_SLUG SILO_GITHUB_CLIENT_ID SILO_GITHUB_CLIENT_SECRET
N=0; BAD=0
ok() { N=$((N+1)); if [ "$2" = 0 ]; then echo "PASS $1"; else echo "FAIL $1 ${3:-}"; BAD=$((BAD+1)); fi; }
t() { if eval "$2"; then ok "$1" 0; else ok "$1" 1 "${3:-}"; fi; }

R=$W/repo; mkdir -p $R/app/SiloUI/src-tauri $R/reviewer-service $R/tools/agent
git -C $R init -q -b main
echo a >$R/app/SiloUI/f; echo t >$R/app/SiloUI/src-tauri/Cargo.toml; echo pytest >$R/reviewer-service/requirements-dev.txt
cat >$R/tools/agent/runtime-cache <<'S'
#!/bin/sh
d=$(cd "$(dirname "$0")/../.." && pwd)
if [ "$1" = --status ]; then echo "key k"; echo "${RC_STATUS:-hit}"; exit 0; fi
mkdir -p "$d/app/SiloUI/src-tauri/binaries" && echo cache >"$d/app/SiloUI/src-tauri/binaries/from-cache"
echo "runtime-cache: hit k"
S
chmod +x $R/tools/agent/runtime-cache
git -C $R add -A; git -C $R commit -qm init
SHA=$(git -C $R rev-parse HEAD)
S=$R/app/SiloUI
mkdir -p $S/src-tauri/binaries $S/src-tauri/runtime; echo msb >$S/src-tauri/binaries/msb; echo rt >$S/src-tauri/runtime/r
echo SECRETCONTENT >$S/github-build.local.json; mkdir -p $S/node_modules

SUITE=$(python3 -I - $A/merge-batch <<'P'
import importlib.machinery, importlib.util, sys
l = importlib.machinery.SourceFileLoader("mb", sys.argv[1]); s = importlib.util.spec_from_loader("mb", l)
m = importlib.util.module_from_spec(s); l.exec_module(m); print(m.SUITE)
P
)
mkdir -p $W/bin
for c in npx npm; do printf '#!/bin/sh\nexit 0\n' >$W/bin/$c; chmod +x $W/bin/$c; done
cat >$W/bin/cargo <<S
#!/bin/sh
echo "bin=\$(ls src-tauri/binaries 2>/dev/null | tr '\n' ,) rt=\$(ls src-tauri/runtime 2>/dev/null | tr '\n' ,) file=\$([ -f github-build.local.json ] && echo yes || echo no) slug=\${SILO_GITHUB_APP_SLUG:-}" >>$W/cargo.out
exit 0
S
cat >$W/bin/uname <<'S'
#!/bin/sh
echo "${FAKE_UNAME:-Darwin}"
S
cat >$W/bin/python3 <<S
#!/bin/sh
echo "\$*" >>$W/py.log
case "\$1" in
  -c) exit \${PYTEST_RC:-0};;
  -m) if [ "\$2" = venv ]; then
        [ -n "\$VENV_FAIL" ] && exit 1
        mkdir -p "\$3/bin"; printf '#!/bin/sh\necho "venv \$*" >>$W/py.log\nexit 0\n' >"\$3/bin/python"; chmod +x "\$3/bin/python"
      fi;;
esac
exit 0
S
chmod +x $W/bin/*
B=$S/target/agent/full-suite.$SHA; mkdir -p $S/target/agent
run() {   # run ENV... : one suite launch; leaves its log in $B.log and verdict in $B.done
  rm -f $B.done $B.log $B.running $W/cargo.out $W/py.log; echo $$ >$B.running; mkdir -p $W/wt.$N
  env "$@" PATH=$W/bin:$PATH bash -c "$SUITE" x $R $B $SHA $W/wt.$N >/dev/null 2>&1
  : >>$W/py.log
}

run RC_STATUS=hit
t "a runtime-cache hit restores the runtime inside the suite worktree and resources it lacks are cloned from the checkout" 'grep -q "bin=from-cache,msb, rt=r," $W/cargo.out' "$(cat $W/cargo.out)"
run RC_STATUS=miss
t "a runtime-cache miss never prepares: the runtime is cloned from the checkout" 'grep -q "bin=msb, rt=r," $W/cargo.out && ! grep -q from-cache $W/cargo.out' "$(cat $W/cargo.out) $(cat $B.log | tail -3)"
t "the github-build.local.json of the checkout is copied and not printed" 'grep -q "file=yes slug=$" $W/cargo.out && ! grep -rq SECRETCONTENT $B.log $B.done' "$(cat $W/cargo.out)"
mv $S/github-build.local.json $W/gb; mv $S/src-tauri/binaries $W/binaries.saved
run RC_STATUS=miss
t "without the build file the synthetic GitHub values are used" 'grep -q "file=no slug=silo-ci-test" $W/cargo.out' "$(cat $W/cargo.out)"
t "without any prepared runtime the log says so" 'grep -q "no prepared native runtime" $B.log'
mv $W/gb $S/github-build.local.json; mv $W/binaries.saved $S/src-tauri/binaries

run FAKE_UNAME=Darwin
t "evals are skipped on macOS, noted in the log and the verdict, not failed" '! grep -q "docs/evals" $W/py.log && grep -q "== evals: skipped" $B.log && [ "$(head -1 $B.done)" = PASS ] && grep -q "^skipped: evals " $B.done' "$(cat $B.done)"
run FAKE_UNAME=Linux
t "evals run on Linux" 'grep -q "unittest discover -s ../../docs/evals" $W/py.log && ! grep -q "^skipped: evals" $B.done'

run PYTEST_RC=0
t "reviewer-service uses the system python when pytest imports" 'grep -q "^-m pytest -q ../../reviewer-service" $W/py.log && ! grep -q -e "-m venv" $W/py.log'
run PYTEST_RC=1
t "without pytest a venv is built from requirements-dev.txt and pytest runs in it" 'grep -q "^-m venv" $W/py.log && grep -q "^venv -m pip install --quiet -r ../../reviewer-service/requirements-dev.txt" $W/py.log && grep -q "^venv -m pytest -q ../../reviewer-service" $W/py.log && [ "$(head -1 $B.done)" = PASS ]' "$(cat $W/py.log)"
run PYTEST_RC=1
t "the venv is cached for the next suite" '! grep -q -e "-m venv" $W/py.log && grep -q "^venv -m pytest" $W/py.log'
rm -rf $SILO_AGENT_CACHE
run PYTEST_RC=1 VENV_FAIL=1
t "when no venv can be built reviewer-service is skipped with a note, not failed" '[ "$(head -1 $B.done)" = PASS ] && grep -q "^skipped: reviewer-service " $B.done && grep -q "== reviewer-service: skipped" $B.log' "$(cat $B.done)"

# merge-batch --last: failure recognition
rm -f $B.*; printf 'FAIL\nfailed: rust vitest script-tests evals reviewer-service release-tests\nskipped: reviewer-service (x)\nsha %s\n' $SHA >$B.done
printf 'app/SiloUI/src-tauri/engine/src/containers/archive.rs\n' >/dev/null
printf 'branch-a\tapp/SiloUI/scripts/test_pkg.py\nbranch-b\tapp/SiloUI/src-tauri/engine/src/containers/archive.rs\nbranch-c\tdocs/other.md\n' >$B.merged
cat >$B.log <<'L'
== rust
   Compiling silo-ui v0.13.0 (/tmp/x/app/SiloUI/src-tauri)
error: failed to run custom build command for `silo-ui v0.13.0 (/tmp/x/app/SiloUI/src-tauri)`

  thread 'main' (1) panicked at src-tauri/build.rs:198:6:
  failed to build desktop permissions: resource path `binaries/msb-aarch64-apple-darwin` doesn't exist
error: command `cargo test --no-run` exited with code 101
        FAIL [   0.123s] silo-engine containers::archive::tests::roundtrip
        FAIL [   0.123s] silo-engine containers::archive::tests::roundtrip
test engine::tests::old_style ... FAILED
== vitest
 FAIL  src/a/b.test.ts > suite > does the thing
 × src/c.spec.tsx
== script-tests
::group::test_pkg.py (FAILED)
======================================================================
FAIL: test_main_builds (test_pkg.PackageTests.test_main_builds) (file='app/SiloUI/src-tauri/engine/src/containers/archive.rs')
----------------------------------------------------------------------
ERROR: test_other (test_pkg.PackageTests.test_other)
----------------------------------------------------------------------
::endgroup::
== evals
FAIL: test_handshake (test_silo_eval.WireFormatTests.test_handshake)
== reviewer-service
FAILED reviewer-service/tests/test_api.py::test_health - assert 1 == 2
== release-tests
✖ metadata CLI works (3.2ms)
ℹ fail 1
✖ failing tests:
L
( cd $R && $A/merge-batch --last ) >$W/last 2>&1; rc=$?
t "--last exits 1 and prints the verdict and skip line" '[ $rc = 1 ] && grep -q "^FAIL" $W/last && grep -q "^skipped: reviewer-service" $W/last' "$(cat $W/last)"
t "--last names a cargo build failure with its reason" 'grep -q "\[rust\] failed to run custom build command for .silo-ui v0.13.0. -- failed to build desktop permissions: resource path" $W/last' "$(cat $W/last)"
t "--last names nextest and cargo test failures once each" '[ "$(grep -c "containers::archive::tests::roundtrip" $W/last)" = 1 ] && grep -q "engine::tests::old_style" $W/last'
t "--last names vitest failures" 'grep -q "\[vitest\] suite > does the thing  (file src/a/b.test.ts)" $W/last && grep -q "\[vitest\] src/c.spec.tsx" $W/last'
t "--last names Python FAIL and ERROR tests under their group" 'grep -q "\[script-tests/test_pkg.py\] test_main_builds (test_pkg.PackageTests)" $W/last && grep -q "\[script-tests/test_pkg.py\] test_other (test_pkg.PackageTests)" $W/last' "$(cat $W/last)"
t "--last blames the branch that touched the test file or the file the test names" 'grep "test_main_builds" $W/last | grep -q "branch-a, branch-b\|branch-b, branch-a" && grep "test_other" $W/last | grep -q "<- branch-a$"' "$(grep test_ $W/last)"
t "--last names evals, pytest and node --test failures" 'grep -q "\[evals\] test_handshake" $W/last && grep -q "\[reviewer-service\] test_health\|\[reviewer-service\] reviewer-service/tests/test_api.py::test_health" $W/last && grep -q "\[release-tests\] metadata CLI works" $W/last && ! grep -q "failing tests:  " $W/last'
printf 'FAIL\nfailed: updater-tests\nsha x\n' >$B.done; printf '== updater-tests\nboom\n' >$B.log
( cd $R && $A/merge-batch --last ) >$W/last 2>&1
t "--last names a failing step that names no test, never (none recognised)" 'grep -q "^  \[updater-tests\] (step failed; no test names recognised" $W/last && ! grep -q "none recognised;" $W/last' "$(cat $W/last)"

echo "$N tests, $BAD failed"; [ $BAD = 0 ]
