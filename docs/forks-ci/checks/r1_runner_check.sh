#!/bin/sh
# Live check for phase R1 (silo-runner). NOT run by CI and not run when it was written.
#
# Needs: the Dev engine running, ~/.local/bin/silo-dev, a Go toolchain, the gh CLI signed in, and a
# PRIVATE test repository you can push workflows to. Nothing here reads or prints a credential:
# GitHub authentication reaches silo-runner through the environment variable named below.
#
#   R1_REPO=OWNER/PRIVATE_REPO R1_TOKEN_ENV=SILO_RUNNER_TOKEN sh docs/forks-ci/checks/r1_runner_check.sh
#
# R1_OS=macos checks the silo-macos recipe (label silo-macos-r1) instead of the default linux one. A
# Mac runs two macOS virtual machines at most: stop other macOS computers first.
#
# SILO_RUNNER_TOKEN must hold a fine-grained personal access token for that repository
# (Administration: read and write, Actions: read, Metadata: read). It takes 15-30 minutes: the
# first recipe build downloads the Linux image and GitHub's runner.
set -eu

: "${R1_REPO:?set R1_REPO to OWNER/REPO of a private test repository}"
R1_TOKEN_ENV=${R1_TOKEN_ENV:-SILO_RUNNER_TOKEN}
R1_OS=${R1_OS:-linux}
case "$R1_OS" in
  linux) max_runners=2; keep=2; os_step='run: |
          printf "#include <stdio.h>\\nint main(void){puts(\"built on a Silo computer\");return 0;}\\n" > hello.c
          cc -o hello hello.c && ./hello' ;;
  macos) max_runners=1; keep=1; os_step='run: sw_vers' ;;
  *) echo "R1_OS must be linux or macos" >&2; exit 1 ;;
esac
label=silo-$R1_OS-r1
eval "token=\${$R1_TOKEN_ENV:-}"
[ -n "$token" ] || { echo "the environment variable $R1_TOKEN_ENV is empty" >&2; exit 1; }
unset token

root=$(cd "$(dirname "$0")/../../.." && pwd)
work=${R1_WORK:-$(mktemp -d "${TMPDIR:-/tmp}/r1-runner.XXXXXX")}
silo="$HOME/.local/bin/silo-dev"
runner_log="$work/runner.log"
pass=0
fail=0
runner_pid=""

ok() { pass=$((pass + 1)); echo "PASS: $1"; }
bad() { fail=$((fail + 1)); echo "FAIL: $1" >&2; }
check() { # description, command...
  desc=$1; shift
  if "$@" > "$work/last.out" 2>&1; then ok "$desc"; else bad "$desc"; sed 's/^/    /' "$work/last.out" >&2; fi
}

cleanup() {
  if [ -n "$runner_pid" ]; then
    kill -INT "$runner_pid" 2> /dev/null || true # graceful: silo-runner deregisters idle runners
    wait "$runner_pid" 2> /dev/null || true
  fi
}
trap cleanup EXIT

visibility=$(gh api "repos/$R1_REPO" --jq .visibility)
[ "$visibility" = private ] || { echo "$R1_REPO is $visibility; this check needs a private repository" >&2; exit 1; }

others_before="$work/others-before.txt"
# not the header (its column widths follow the longest name) and not the recipe's builder computer,
# which this script itself makes
others() { "$silo" list --all 2> /dev/null | grep -v -e '^ci-' -e '^NAME' -e "^silo-build-silo-$R1_OS-" | sort; }
others > "$others_before"

echo "== build silo-runner"
(cd "$root/app/SiloUI/runner" && go build -o "$work/silo-runner" .)

cat > "$work/config.yaml" << YAML
github:
  configUrl: https://github.com/$R1_REPO
  auth: {tokenEnv: $R1_TOKEN_ENV}
silo:
  path: $silo
  projectDir: $work/project
stateDir: $work/state
limits: {readyTimeout: 5m}
runnerRefresh: {disabled: true}
scaleSets:
  - {label: $label, recipe: silo-$R1_OS, os: $R1_OS, maxRunners: $max_runners, keepFailed: {count: $keep, days: 1}}
YAML

check "config is valid and the repository is private" "$work/silo-runner" check -config "$work/config.yaml"
check "recipes install" "$work/silo-runner" install-recipes -config "$work/config.yaml"
echo "== build the recipe (slow the first time)"
(cd "$work/project" && "$silo" recipe build "silo-$R1_OS")

echo "== start silo-runner"
"$work/silo-runner" run -config "$work/config.yaml" > "$runner_log" 2>&1 &
runner_pid=$!
sleep 5
check "silo-runner is running" kill -0 "$runner_pid"
check "a second silo-runner on the same state is refused" sh -c "'$work/silo-runner' run -config '$work/config.yaml' 2>&1 | grep -q 'another silo-runner'"

dispatch() { # name, script (@LABEL@ is the scale set's label)
  body=$(printf '%s' "$2" | sed "s/@LABEL@/$label/g")
  # the sha of an existing file is needed to replace it; an empty repository or a new file has none
  sha=$(gh api "repos/$R1_REPO/contents/.github/workflows/$1.yml" --jq .sha 2> /dev/null) || sha=""
  if [ -n "$sha" ]; then
    gh api -X PUT "repos/$R1_REPO/contents/.github/workflows/$1.yml" -f message="R1 check: $1" \
      -f content="$(printf '%s' "$body" | base64 | tr -d '\n')" -f sha="$sha" > /dev/null
  else
    gh api -X PUT "repos/$R1_REPO/contents/.github/workflows/$1.yml" -f message="R1 check: $1" \
      -f content="$(printf '%s' "$body" | base64 | tr -d '\n')" > /dev/null
  fi
}
run_workflow() { # name -> prints conclusion
  gh workflow run "$1.yml" -R "$R1_REPO" > /dev/null
  sleep 10
  id=$(gh run list -R "$R1_REPO" --workflow "$1.yml" --limit 1 --json databaseId --jq '.[0].databaseId')
  echo "$id" > "$work/last-run-id"
  gh run watch "$id" -R "$R1_REPO" > /dev/null 2>&1 || true
  gh run view "$id" -R "$R1_REPO" --json conclusion --jq .conclusion
}

runner_name_of_last_run() { gh api "repos/$R1_REPO/actions/runs/$(cat "$work/last-run-id")/jobs" --jq '.jobs[0].runner_name'; }
github_has_runner() { gh api "repos/$R1_REPO/actions/runners" --jq '.runners[].name' | grep -qx "$1"; }
computer_line() { "$silo" list --all 2> /dev/null | awk -v n="$1" '$1 == n'; }
wait_gone() { # computer name: wait up to 4 minutes for the computer to leave `silo list`
  n=0
  while [ -n "$(computer_line "$1")" ] && [ $n -lt 48 ]; do sleep 5; n=$((n + 1)); done
  [ -z "$(computer_line "$1")" ]
}
wait_stopped() { # computer name: wait up to 4 minutes for the computer to be listed as stopped
  n=0
  while ! computer_line "$1" | grep -q stopped && [ $n -lt 48 ]; do sleep 5; n=$((n + 1)); done
  computer_line "$1" | grep -q stopped
}

dispatch r1-ok 'name: r1-ok
on: workflow_dispatch
jobs:
  j:
    runs-on: @LABEL@
    steps:
      - run: echo "hostname=$(hostname) runner=$RUNNER_NAME"
      - run: uname -a
      - '"$os_step"'
'
dispatch r1-fail 'name: r1-fail
on: workflow_dispatch
jobs:
  j:
    runs-on: @LABEL@
    steps:
      - run: exit 3
'

# Sample the host's process list while the jobs run: the JIT configuration (it starts with eyJ) must
# be in no command line of silo, silo-runner or the runner program. The sampler must also have seen
# the runner's own command (`exec --stdin`), or it saw nothing.
: > "$work/jit-seen"
: > "$work/jit-leak"
touch "$work/sampler-on"
(
  while [ -e "$work/sampler-on" ]; do
    ps -axo command | grep -E 'silo-dev|silo-runner|silo-run-jit|actions-runner|Runner\.' | grep -v grep > "$work/ps-now" || true
    grep -q -e '--stdin' "$work/ps-now" && echo x >> "$work/jit-seen"
    grep -q 'eyJ' "$work/ps-now" && echo x >> "$work/jit-leak"
    sleep 0.5
  done
) &

echo "== a job that succeeds"
[ "$(run_workflow r1-ok)" = success ] && ok "the job ran on a Silo computer and succeeded" || bad "r1-ok did not succeed"
ok_runner=$(runner_name_of_last_run)
echo "    the job page names the runner: $ok_runner"
case "$ok_runner" in "ci-$label"-*) ok "the runner is named like a ci-$label computer" ;; *) bad "the runner name $ok_runner is not a ci-$label computer name" ;; esac
check "the job's log shows the same runner name" sh -c "gh run view '$(cat "$work/last-run-id")' -R '$R1_REPO' --log | grep -q 'runner=$ok_runner'"
check "the successful job's computer was deleted" wait_gone "$ok_runner"
check "its runner is gone from GitHub" sh -c "! gh api 'repos/$R1_REPO/actions/runners' --jq '.runners[].name' | grep -qx '$ok_runner'"

echo "== a job that fails"
[ "$(run_workflow r1-fail)" = failure ] && ok "the failing job failed" || bad "r1-fail did not fail"
fail_runner=$(runner_name_of_last_run)
echo "    the job page names the runner: $fail_runner"
[ "$fail_runner" != "$ok_runner" ] && ok "each job got its own computer ($ok_runner, then $fail_runner)" || bad "both jobs ran on $ok_runner"
check "the failed job's computer is kept, stopped" wait_stopped "$fail_runner"
check "the failed job's runner is gone from GitHub" sh -c "! gh api 'repos/$R1_REPO/actions/runners' --jq '.runners[].name' | grep -qx '$fail_runner'"
echo "    ci-* computers now:"; "$silo" list --all | grep '^ci-' | sed 's/^/    /' || true

echo "== no JIT configuration in any process command line or in the log"
rm -f "$work/sampler-on"
sleep 1
echo "    the sampler saw the runner command $(wc -l < "$work/jit-seen" | tr -d ' ') times"
check "the sampler saw the runner's exec --stdin command" sh -c "[ -s '$work/jit-seen' ]"
check "no JIT configuration on any silo or runner command line while the jobs ran" sh -c "[ ! -s '$work/jit-leak' ]"
check "the log has no encoded JIT config" sh -c "! grep -q 'eyJ' '$runner_log'"

echo "== restart reconcile"
cleanup
runner_pid=""
"$work/silo-runner" run -config "$work/config.yaml" >> "$runner_log" 2>&1 &
runner_pid=$!
sleep 15
check "the kept failure survives a restart, still stopped" sh -c "'$silo' list --all | awk -v n='$fail_runner' '\$1 == n' | grep -q stopped"
check "the restart made no other ci-* computer" sh -c "[ \"\$('$silo' list --all | grep -c '^ci-')\" = 1 ]"

others > "$work/others-after.txt"
check "no computer other than ci-* changed on the engine" diff "$others_before" "$work/others-after.txt"

echo "== clean up the test repository workflows and computers"
for name in r1-ok r1-fail; do
  sha=$(gh api "repos/$R1_REPO/contents/.github/workflows/$name.yml" --jq .sha 2> /dev/null || true)
  [ -z "$sha" ] || gh api -X DELETE "repos/$R1_REPO/contents/.github/workflows/$name.yml" -f message="R1 check: remove $name" -f sha="$sha" > /dev/null || true
done
cleanup
runner_pid=""
for name in $("$silo" list --all --json | grep -o "\"name\": *\"ci-$label-[^\"]*\"" | sed 's/.*"\(ci-[^"]*\)"/\1/'); do
  "$silo" drop "$name" > /dev/null 2>&1 || true
done

echo "passed: $pass, failed: $fail; work dir: $work"
[ "$fail" -eq 0 ]
