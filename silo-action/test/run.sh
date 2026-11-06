#!/bin/sh
# Runs ../run.sh against the fake silo. Usage: sh silo-action/test/run.sh
here=$(cd "$(dirname "$0")" && pwd)
pass=0
failn=0
check() { if eval "$2"; then pass=$((pass + 1)); else failn=$((failn + 1)); echo "FAIL: $1"; fi; }

# run_case NAME [VAR=VALUE]... sets code, log, sum, outp and err.
run_case() {
  shift
  d=$(mktemp -d)
  : > "$d/log"
  : > "$d/sum"
  : > "$d/out"
  env -i PATH="$PATH" HOME="$d" RUNNER_TEMP="$d" GITHUB_RUN_ID=42 GITHUB_RUN_ATTEMPT=1 \
    GITHUB_STEP_SUMMARY="$d/sum" GITHUB_OUTPUT="$d/out" GITHUB_TOKEN=tok \
    FAKE_SILO_LOG="$d/log" INPUT_SILO_PATH="$here/silo" INPUT_RUN="make test" "$@" \
    sh "$here/../run.sh" > "$d/stdout" 2> "$d/stderr"
  code=$?
  log=$(cat "$d/log")
  sum=$(cat "$d/sum")
  outp=$(cat "$d/out")
  err=$(cat "$d/stderr")
  rm -rf "$d"
}

run_case pr INPUT_FOLD=pr INPUT_FROM=main INPUT_SECRETS="NPM_TOKEN, API_KEY"
check "pr: exit 0" '[ $code -eq 0 ]'
check "pr: fork from main with secrets and command" 'echo "$log" | grep -q "^fork main --name gha-42-1 --secret NPM_TOKEN --secret API_KEY -- sh -c make test"'
check "pr: fold --pr called" 'echo "$log" | grep -q "^fold gha-42-1 --pr"'
check "pr: summary has buckets" 'echo "$sum" | grep -q "\*\*Code\*\*" && echo "$sum" | grep -q "jq 1.7.1"'
check "pr: summary has credentials line" 'echo "$sum" | grep -q "Credentials touched"'
check "pr: outputs" 'echo "$outp" | grep -q "^pr-url=https://example.invalid/o/r/pull/7" && echo "$outp" | grep -q "^fork=gha-42-1" && echo "$outp" | grep -q "^exit-code=0"'
check "pr: fork dropped" 'echo "$log" | grep -q "^drop gha-42-1"'

run_case recipe INPUT_FOLD=pr INPUT_RECIPE=linux-dev
check "recipe: silo up then fork main" 'echo "$log" | grep -q "^up --recipe linux-dev" && echo "$log" | grep -q "^fork main --name gha-42-1"'

run_case fail INPUT_FOLD=pr FAKE_SILO_EXIT=3
check "fail: exit code propagated" '[ $code -eq 3 ]'
check "fail: summary written" 'echo "$sum" | grep -q "exited with 3" && echo "$sum" | grep -q "\*\*Code\*\*"'
check "fail: not folded" '! echo "$log" | grep -q "^fold"'
check "fail: fork dropped" 'echo "$log" | grep -q "^drop gha-42-1"'
check "fail: exit-code output" 'echo "$outp" | grep -q "^exit-code=3"'

run_case request INPUT_FOLD=request
check "request: exit 0 and id" '[ $code -eq 0 ] && echo "$outp" | grep -q "^request-id=fr-0a1b2c3d"'
check "request: fold --request" 'echo "$log" | grep -q "^fold gha-42-1 --request" && ! echo "$log" | grep -q -- "--pr"'

run_case keep INPUT_FOLD=none INPUT_KEEP_FORK=true
check "keep-fork: not dropped, not folded" '[ $code -eq 0 ] && ! echo "$log" | grep -q "^drop" && ! echo "$log" | grep -q "^fold"'

run_case missing INPUT_SILO_PATH="$here/does-not-exist"
check "missing silo: fails with message" '[ $code -ne 0 ] && echo "$err" | grep -q "silo not found"'

run_case down FAKE_SILO_ENGINE=down
check "engine down: fails with message" '[ $code -ne 0 ] && echo "$err" | grep -q "engine does not answer" && ! echo "$log" | grep -q "^fork"'

echo "silo-action tests: $pass passed, $failn failed"
[ "$failn" -eq 0 ]
