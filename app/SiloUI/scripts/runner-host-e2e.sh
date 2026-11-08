#!/usr/bin/env bash
# Checks `silo-runner` from an engine archive on a real Linux host with a systemd user manager and
# KVM. It never contacts GitHub: registering runners needs an admin token, which CI does not hold.
# What it does prove: the binary in the archive runs on this architecture, `check -offline` finds
# /dev/kvm and sizes the Linux budget from the host, `service register` writes a user unit that
# starts after the engine's, bringing the engine up when it is down, the helper runs as this
# account, its log lines land in the user journal, and `service unregister` removes the unit.
# The jobs themselves (JIT runners, a computer per job) are docs/forks-ci/checks/r2_linux_runner_check.sh.
#
# Usage: runner-host-e2e.sh ENGINE_DIR ENGINE_UNIT
# ENGINE_DIR is an unpacked engine archive whose engine ENGINE_UNIT (a systemd user unit) is serving.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 ENGINE_DIR ENGINE_UNIT" >&2
  exit 2
fi
engine="$1"
engine_unit="$2"
runner="$engine/silo-runner"
unit=silo-runner.service
work="$(mktemp -d)"

step() { printf '\n== %s\n' "$*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

cleanup() {
  local status=$?
  trap - EXIT
  "$runner" service unregister > /dev/null 2>&1 || true
  rm -rf "$work"
  exit "$status"
}
trap cleanup EXIT

step "the binary in the archive"
version="$("$runner" version)"
echo "silo-runner $version"
[[ -n "$version" && "$version" != dev ]] || fail "the archive's silo-runner has no version"
readelf -h "$runner" | grep -q "Machine:.*$(uname -m | sed 's/aarch64/AArch64/; s/x86_64/X86-64/')" || fail "silo-runner is not built for $(uname -m)"
test -s "$engine/resources/silo-runner-licenses/scaleset-MIT.txt" || fail "no licenses beside silo-runner"

cat > "$work/config.yaml" << YAML
github:
  configUrl: https://github.com/example/private-repo
  auth: {tokenEnv: SILO_RUNNER_TOKEN}
silo:
  path: $engine/silo
  projectDir: $work/project
stateDir: $work/state
scaleSets:
  - {label: silo-linux, recipe: silo-linux, os: linux}
YAML

step "check -offline on this machine"
"$runner" check -offline -config "$work/config.yaml" | tee "$work/check.txt"
grep -q "(from this host)" "$work/check.txt" || fail "the budget did not come from the host"
cpus="$(nproc)"
grep -q "at most $cpus at once" "$work/check.txt" || fail "the count is not one per CPU ($cpus)"

step "a Mac scale set is refused on a Linux host"
sed 's/os: linux/os: macos/' "$work/config.yaml" > "$work/macos.yaml"
if "$runner" check -offline -config "$work/macos.yaml" 2> "$work/macos.err"; then fail "a macOS scale set was accepted"; fi
grep -q "cannot run macOS" "$work/macos.err" || fail "unexpected refusal: $(cat "$work/macos.err")"

step "register the service"
"$runner" service register -config "$work/config.yaml" -engine-unit "$engine_unit" | tee "$work/register.txt"
grep -q "journalctl --user -u $unit" "$work/register.txt" || fail "register did not name the journal"
systemctl --user is-enabled "$unit"
after="$(systemctl --user show "$unit" --property=After --value)"
wants="$(systemctl --user show "$unit" --property=Wants --value)"
[[ " $after " == *" $engine_unit "* ]] || fail "the unit is not ordered after $engine_unit: $after"
[[ " $wants " == *" $engine_unit "* ]] || fail "the unit does not want $engine_unit: $wants"
systemd-analyze --user verify "$HOME/.config/systemd/user/$unit"

step "the helper runs as this account and logs to the journal"
# No token is set, so the helper refuses to start, says why and is restarted: the unit and the
# journal are what is under test, not GitHub.
for _ in $(seq 30); do
  journalctl --user -u "$unit" --no-pager 2> /dev/null | grep -q "SILO_RUNNER_TOKEN, which is empty" && break
  sleep 1
done
journalctl --user -u "$unit" --no-pager | tail -n 5
journalctl --user -u "$unit" --no-pager | grep -q "SILO_RUNNER_TOKEN, which is empty" || fail "the helper's message is not in the user journal"
# The journal records the uid of the process that wrote each line.
journalctl --user -u "$unit" --no-pager -o verbose | grep -q "^ *_UID=$(id -u)$" || fail "the helper's lines were not written by uid $(id -u)"

step "starting the helper brings the engine up"
systemctl --user stop "$unit"
systemctl --user stop "$engine_unit"
systemctl --user is-active "$engine_unit" && fail "the engine did not stop"
systemctl --user start "$unit"
for _ in $(seq 60); do
  systemctl --user is-active --quiet "$engine_unit" && break
  sleep 1
done
systemctl --user is-active --quiet "$engine_unit" || fail "starting $unit did not start $engine_unit"
# The unit is Type=simple, so "active" comes before the socket answers.
for _ in $(seq 60); do
  "$engine/silo" list > /dev/null 2>&1 && break
  sleep 1
done
"$engine/silo" list > /dev/null || fail "the engine does not answer"

step "unregister"
"$runner" service unregister
test ! -e "$HOME/.config/systemd/user/$unit" || fail "the unit file is still there"
systemctl --user is-active --quiet "$engine_unit" || fail "unregistering the helper stopped the engine"
echo "ok"
