#!/bin/sh
# Live check for phase R2 (silo-runner on a Linux host). NOT run by CI and NOT run when it was written:
# it needs a Linux machine with the engine installed from an archive, KVM, and a GitHub token that can
# register runners, which CI does not hold.
#
# Run it ON the Linux host, as the account that runs silod (not root), after `silo host init`, with:
#   - the archive's silo-runner (default ~/.local/opt/silo-engine/silo-runner; set R2_ENGINE_DIR),
#   - `silo` on PATH, jq, the gh CLI signed in,
#   - a PRIVATE test repository you can push workflows to,
#   - no silo-runner.service already registered for this account (the check registers and removes it).
#
#   R2_REPO=OWNER/PRIVATE_REPO SILO_RUNNER_TOKEN=... sh docs/forks-ci/checks/r2_linux_runner_check.sh
#
# The token needs: Administration read and write, Actions read, Metadata read on that repository. It is
# written to ~/.config/silo-runner/env (mode 0600) for the service and removed again at the end; nothing
# prints it. The first run builds the silo-linux recipe, which takes 15-30 minutes. What it checks:
#   1. the runner installed for this machine's architecture (x64 or arm64) and the job saw it;
#   2. a job that succeeds runs on its own computer, which is deleted; its runner leaves GitHub;
#   3. a job that fails keeps its computer, stopped; its runner leaves GitHub;
#   4. the service: ordered after the engine, running as this account, logging to the user journal, no JIT
#      configuration in the journal or in any command line;
#   5. the service stops cleanly and leaves no idle runner at GitHub or computer behind.
set -eu

: "${R2_REPO:?set R2_REPO to OWNER/REPO of a private test repository}"
R2_TOKEN_ENV=${R2_TOKEN_ENV:-SILO_RUNNER_TOKEN}
R2_ENGINE_DIR=${R2_ENGINE_DIR:-$HOME/.local/opt/silo-engine}
R2_ENGINE_UNIT=${R2_ENGINE_UNIT:-org.silo.preview.engine.service}
eval "token=\${$R2_TOKEN_ENV:-}"
[ -n "$token" ] || { echo "the environment variable $R2_TOKEN_ENV is empty" >&2; exit 1; }
[ "$(id -u)" != 0 ] || { echo "run this as the account that runs silod, not root" >&2; exit 1; }
for tool in gh jq silo; do command -v "$tool" > /dev/null || { echo "$tool is needed" >&2; exit 1; }; done
runner="$R2_ENGINE_DIR/silo-runner"
[ -x "$runner" ] || { echo "$runner not found; set R2_ENGINE_DIR" >&2; exit 1; }
config_dir="$HOME/.config/silo-runner"
unit_file="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/silo-runner.service"
[ ! -e "$unit_file" ] || { echo "$unit_file exists: unregister your service first (silo-runner service unregister)" >&2; exit 1; }
[ ! -e "$config_dir/env" ] || { echo "$config_dir/env exists: move it away for this check" >&2; exit 1; }

work=${R2_WORK:-$(mktemp -d "${TMPDIR:-/tmp}/r2-runner.XXXXXX")}
label=silo-linux-r2
unit=silo-runner.service
pass=0
fail=0
ok() { pass=$((pass + 1)); echo "PASS: $1"; }
bad() { fail=$((fail + 1)); echo "FAIL: $1" >&2; }
check() { # description, command...
  desc=$1; shift
  if "$@" > "$work/last.out" 2>&1; then ok "$desc"; else bad "$desc"; sed 's/^/    /' "$work/last.out" >&2; fi
}

cleanup() {
  rm -f "$work/sampler-on"
  "$runner" service unregister > /dev/null 2>&1 || true
  rm -f "$config_dir/env"
  rmdir "$config_dir" 2> /dev/null || true
  for name in r2-ok r2-fail; do
    sha=$(gh api "repos/$R2_REPO/contents/.github/workflows/$name.yml" --jq .sha 2> /dev/null || true)
    [ -z "$sha" ] || gh api -X DELETE "repos/$R2_REPO/contents/.github/workflows/$name.yml" -f message="R2 check: remove $name" -f sha="$sha" > /dev/null 2>&1 || true
  done
  for name in $(silo list --all --json 2> /dev/null | jq -r ".computers[].name | select(startswith(\"ci-$label-\"))"); do
    silo stop "$name" > /dev/null 2>&1 || true
    silo drop "$name" > /dev/null 2>&1 || true
  done
}
trap cleanup EXIT

visibility=$(gh api "repos/$R2_REPO" --jq .visibility)
[ "$visibility" = private ] || { echo "$R2_REPO is $visibility; this check needs a private repository" >&2; exit 1; }

others() { silo list --all --json | jq -r '.computers[] | select(.name | startswith("ci-") or startswith("silo-build-silo-linux-") | not) | [.id, .name, .state] | @tsv' | sort; }
others > "$work/others-before.txt"

case "$(uname -m)" in
  x86_64) want_arch=X64 ;;
  aarch64) want_arch=ARM64 ;;
  *) echo "unsupported architecture $(uname -m)" >&2; exit 1 ;;
esac

mkdir -p "$config_dir"
chmod 700 "$config_dir"
umask 077
printf '%s=%s\n' "$R2_TOKEN_ENV" "$token" > "$config_dir/env"
unset token
cat > "$work/config.yaml" << YAML
github:
  configUrl: https://github.com/$R2_REPO
  auth: {tokenEnv: $R2_TOKEN_ENV}
silo:
  path: $(command -v silo)
  projectDir: $work/project
stateDir: $work/state
limits: {readyTimeout: 5m}
runnerRefresh: {disabled: true}
scaleSets:
  - {label: $label, recipe: silo-linux, os: linux, maxRunners: 2, keepFailed: {count: 2, days: 1}}
YAML

check "offline check: /dev/kvm and the budget" "$runner" check -offline -config "$work/config.yaml"
check "recipes install" "$runner" install-recipes -config "$work/config.yaml"
echo "== build the recipe (slow the first time)"
(cd "$work/project" && silo recipe build silo-linux)

echo "== register and start the service"
"$runner" service register -config "$work/config.yaml" -engine-unit "$R2_ENGINE_UNIT"
sleep 10
check "the service is active" systemctl --user is-active --quiet "$unit"
check "it is ordered after the engine" sh -c "systemctl --user show $unit --property=After --value | tr ' ' '\\n' | grep -qx '$R2_ENGINE_UNIT'"
check "the journal has its capacity line, from this host" sh -c "journalctl --user -u $unit --no-pager | grep -q 'Linux computers: at most'"
check "the journal says it is serving" sh -c "journalctl --user -u $unit --no-pager | grep -q 'silo-runner is serving'"

dispatch() { # name, script (@LABEL@ is the scale set's label)
  body=$(printf '%s' "$2" | sed "s/@LABEL@/$label/g")
  sha=$(gh api "repos/$R2_REPO/contents/.github/workflows/$1.yml" --jq .sha 2> /dev/null) || sha=""
  if [ -n "$sha" ]; then
    gh api -X PUT "repos/$R2_REPO/contents/.github/workflows/$1.yml" -f message="R2 check: $1" -f content="$(printf '%s' "$body" | base64 | tr -d '\n')" -f sha="$sha" > /dev/null
  else
    gh api -X PUT "repos/$R2_REPO/contents/.github/workflows/$1.yml" -f message="R2 check: $1" -f content="$(printf '%s' "$body" | base64 | tr -d '\n')" > /dev/null
  fi
}
run_workflow() { # name -> prints conclusion
  gh workflow run "$1.yml" -R "$R2_REPO" > /dev/null
  sleep 10
  id=$(gh run list -R "$R2_REPO" --workflow "$1.yml" --limit 1 --json databaseId --jq '.[0].databaseId')
  echo "$id" > "$work/last-run-id"
  gh run watch "$id" -R "$R2_REPO" > /dev/null 2>&1 || true
  gh run view "$id" -R "$R2_REPO" --json conclusion --jq .conclusion
}
runner_name_of_last_run() { gh api "repos/$R2_REPO/actions/runs/$(cat "$work/last-run-id")/jobs" --jq '.jobs[0].runner_name'; }
computer_line() { silo list --all 2> /dev/null | awk -v n="$1" '$1 == n'; }
wait_gone() { n=0; while [ -n "$(computer_line "$1")" ] && [ $n -lt 48 ]; do sleep 5; n=$((n + 1)); done; [ -z "$(computer_line "$1")" ]; }
wait_stopped() { n=0; while ! computer_line "$1" | grep -q stopped && [ $n -lt 48 ]; do sleep 5; n=$((n + 1)); done; computer_line "$1" | grep -q stopped; }

dispatch r2-ok 'name: r2-ok
on: workflow_dispatch
jobs:
  j:
    runs-on: @LABEL@
    steps:
      - run: echo "runner=$RUNNER_NAME arch=$RUNNER_ARCH os=$RUNNER_OS machine=$(uname -m)"
      - run: cc --version | head -n 1
'
dispatch r2-fail 'name: r2-fail
on: workflow_dispatch
jobs:
  j:
    runs-on: @LABEL@
    steps:
      - run: exit 3
'

# While the jobs run, no command line may hold the JIT configuration (it starts with eyJ).
: > "$work/jit-seen"
: > "$work/jit-leak"
touch "$work/sampler-on"
(
  while [ -e "$work/sampler-on" ]; do
    ps -eo args | grep -E 'silo|silo-runner|silo-run-jit|actions-runner|Runner\.' | grep -v grep > "$work/ps-now" || true
    grep -q -e '--stdin' "$work/ps-now" && echo x >> "$work/jit-seen"
    grep -q 'eyJ' "$work/ps-now" && echo x >> "$work/jit-leak"
    sleep 0.5
  done
) &

echo "== a job that succeeds"
[ "$(run_workflow r2-ok)" = success ] && ok "the job ran on a Silo computer and succeeded" || bad "r2-ok did not succeed"
ok_runner=$(runner_name_of_last_run)
echo "    runner: $ok_runner"
case "$ok_runner" in "ci-$label"-*) ok "the runner is named like a ci-$label computer" ;; *) bad "unexpected runner name $ok_runner" ;; esac
check "the job saw the runner for this architecture ($want_arch)" sh -c "gh run view '$(cat "$work/last-run-id")' -R '$R2_REPO' --log | grep -q 'arch=$want_arch os=Linux machine=$(uname -m)'"
check "the successful job's computer was deleted" wait_gone "$ok_runner"
check "its runner is gone from GitHub" sh -c "! gh api 'repos/$R2_REPO/actions/runners' --jq '.runners[].name' | grep -qx '$ok_runner'"

echo "== a job that fails"
[ "$(run_workflow r2-fail)" = failure ] && ok "the failing job failed" || bad "r2-fail did not fail"
fail_runner=$(runner_name_of_last_run)
echo "    runner: $fail_runner"
[ "$fail_runner" != "$ok_runner" ] && ok "each job got its own computer" || bad "both jobs ran on $ok_runner"
check "the failed job's computer is kept, stopped" wait_stopped "$fail_runner"
check "the failed job's runner is gone from GitHub" sh -c "! gh api 'repos/$R2_REPO/actions/runners' --jq '.runners[].name' | grep -qx '$fail_runner'"

echo "== the service"
rm -f "$work/sampler-on"
sleep 1
check "the sampler saw the runner's exec --stdin command" sh -c "[ -s '$work/jit-seen' ]"
check "no JIT configuration on any command line while the jobs ran" sh -c "[ ! -s '$work/jit-leak' ]"
check "no JIT configuration in the journal" sh -c "! journalctl --user -u $unit --no-pager | grep -q eyJ"
check "the journal lines are this account's" sh -c "journalctl --user -u $unit --no-pager -o verbose | grep -q '^ *_UID=$(id -u)\$'"
check "the helper process runs as this account" sh -c "[ \"\$(ps -o user= -p \$(systemctl --user show $unit --property=MainPID --value) | tr -d ' ')\" = '$(id -un)' ]"
systemctl --user stop "$unit"
check "the service stopped cleanly" sh -c "[ \"\$(systemctl --user show $unit --property=Result --value)\" = success ]"
check "the kept failure survives the stop, still stopped" sh -c "silo list --all | awk -v n='$fail_runner' '\$1 == n' | grep -q stopped"
check "no idle runner is left at GitHub" sh -c "! gh api 'repos/$R2_REPO/actions/runners' --jq '.runners[].name' | grep -q '^ci-$label-'"
others > "$work/others-after.txt"
check "no computer other than ci-* changed on the engine" diff "$work/others-before.txt" "$work/others-after.txt"

echo "passed: $pass, failed: $fail; work dir: $work"
[ "$fail" -eq 0 ]
