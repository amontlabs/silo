#!/bin/bash
# The Linux analogue of test 41: gvproxy as released, #363's build and #380's build (-sealed,
# -closed), each with a network namespace for the computer, tcpdump watching this machine's
# lookups. Must end silo-holds; then, with gvproxy as released planted in #380's build's place,
# silo-leaks. Skips (exit 0) without root, namespaces, sshd or a network to fetch gvproxy.
set -u
[ "$(uname -s)" = Linux ] || { echo "Linux only: skipped"; exit 0; }
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
tmp="$(mktemp -d /tmp/kit41.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT
# The builds are kept between runs, with test 35's.
export SILO_KIT_HOME="$tmp/home" KIT_WORK_ROOT="${KIT_35_HOME:-${TMPDIR:-/tmp}/silo-kit-35-home}" KIT_ID=41
export KIT_WORK="$tmp/w" KIT_OUT="$tmp/results.jsonl" KIT_FAKE=1 KIT_YES=1
mkdir -p "$KIT_WORK" "$KIT_WORK_ROOT"
: > "$KIT_OUT"
bash "$KIT_ROOT/tests/41-ping-and-names-past-gvproxy.sh" < /dev/null > "$tmp/run.log" 2>&1
rc=$?
if grep -q '"kind":"skip"' "$KIT_OUT"; then echo "test 41's analogue skipped: $(grep '"kind":"skip"' "$KIT_OUT" | head -1)"; exit 0; fi
[ "$rc" = 0 ] || { echo "test 41 exited $rc"; tail -20 "$tmp/run.log"; exit 1; }
grep -q '"key":"silo-holds"' "$KIT_OUT" || { echo "FAIL: expected silo-holds"; grep -E '"kind":"(outcome|fact|note)"' "$KIT_OUT" | tail -20; exit 1; }
echo "ok   41: $(grep '"kind":"outcome"' "$KIT_OUT" | sed 's/.*"text":"//' | cut -c1-220)"
m="$tmp/mutant"; mkdir -p "$m/bin"
cp "$KIT_WORK_ROOT/bin/gvproxy-v0.9.0" "$m/bin/"
silo="$(cd "$KIT_WORK_ROOT/bin" && ls gvproxy-silo-* | head -1)"
cat > "$m/bin/$silo" <<'EOS'
#!/bin/sh
# a planted fault: gvproxy as released, claiming to be #380's build; its flags are dropped
case "$1" in -help) echo "  -outbound-deny"; exit 0 ;; esac
exec "$(dirname "$0")/gvproxy-v0.9.0" "$1" "$2"
EOS
chmod +x "$m/bin/$silo"
: > "$KIT_OUT"; rm -rf "$KIT_WORK"; mkdir -p "$KIT_WORK"
KIT_WORK_ROOT="$m" bash "$KIT_ROOT/tests/41-ping-and-names-past-gvproxy.sh" < /dev/null > "$tmp/mutant.log" 2>&1
if grep -q '"key":"silo-leaks"' "$KIT_OUT"; then
  echo "ok   41 with gvproxy as released in #380's build's place: silo-leaks ($(grep '"kind":"outcome"' "$KIT_OUT" | sed 's/.*"text":"//' | cut -c1-160))"
else
  echo "FAIL: the planted fault was not caught"; grep '"kind":"outcome"' "$KIT_OUT"; tail -20 "$tmp/mutant.log"; exit 1
fi
