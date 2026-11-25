#!/bin/bash
# The Linux analogue of test 35: #363's sealed gvproxy, built from gvisor-tap-vsock v0.9.0 with
# net/gvproxy-sealed.patch, a network namespace for the computer, an OpenSSH server in it. Root in
# the namespace must reach nothing of this machine, and still the internet (through this
# machine's network) and Silo's SSH forward. Skips (exit 0) without root, namespaces, sshd, git or
# a network.
set -u
[ "$(uname -s)" = Linux ] || { echo "Linux only: skipped"; exit 0; }
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
tmp="$(mktemp -d /tmp/kit35.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT
# The build is kept between runs, since it takes a minute.
export SILO_KIT_HOME="$tmp/home" KIT_WORK_ROOT="${KIT_35_HOME:-${TMPDIR:-/tmp}/silo-kit-35-home}" KIT_ID=35
export KIT_WORK="$tmp/w" KIT_OUT="$tmp/results.jsonl" KIT_FAKE=1 KIT_YES=1
mkdir -p "$KIT_WORK" "$KIT_WORK_ROOT"
: > "$KIT_OUT"
bash "$KIT_ROOT/tests/35-sealed-gvproxy.sh" < /dev/null
rc=$?
if grep -q '"kind":"skip"' "$KIT_OUT"; then
  echo "test 35's analogue skipped: $(grep '"kind":"skip"' "$KIT_OUT" | head -1)"; exit 0
fi
[ "$rc" = 0 ] || { echo "test 35 exited $rc"; exit 1; }
grep -q '"key":"sealed-holds"' "$KIT_OUT" || { echo "FAIL: expected sealed-holds"; grep -E '"kind":"(outcome|fact)"' "$KIT_OUT" | tail -25; exit 1; }
echo "ok   35: sealed-holds"

# The planted fault: gvproxy as released, standing in for the sealed build (the sealed line of
# its configuration taken out). The test must see the Mac reached.
m="$tmp/mutant"; mkdir -p "$m/bin"
( . "$KIT_ROOT/lib/common.sh"; . "$KIT_ROOT/lib/net.sh"; net_gvproxy_get ) || { echo "skip the planted fault: gvproxy as released could not be downloaded"; exit 0; }
cp "$KIT_WORK_ROOT/bin/gvproxy-v0.9.0" "$m/bin/"
cat > "$m/bin/gvproxy-v0.9.0-sealed" <<'EOS'
#!/bin/sh
# a planted fault: gvproxy as released, claiming to be the sealed build
case "$1" in -help) echo "  -sealed"; exit 0 ;; -version) exec "$(dirname "$0")/gvproxy-v0.9.0" -version ;; esac
grep -v '^sealed:' "$2" > "$2.unsealed"
exec "$(dirname "$0")/gvproxy-v0.9.0" -config "$2.unsealed"
EOS
chmod +x "$m/bin/gvproxy-v0.9.0-sealed"
: > "$KIT_OUT"; rm -rf "$KIT_WORK"; mkdir -p "$KIT_WORK"
KIT_WORK_ROOT="$m" bash "$KIT_ROOT/tests/35-sealed-gvproxy.sh" < /dev/null > "$tmp/mutant.log" 2>&1
if grep -q '"key":"sealed-leaks"' "$KIT_OUT"; then
  echo "ok   35 with gvproxy as released in the sealed build's place: sealed-leaks ($(grep '"kind":"outcome"' "$KIT_OUT" | sed 's/.*"text":"//' | cut -c1-120))"
else
  echo "FAIL: the planted fault was not caught"; tail -20 "$tmp/mutant.log"; exit 1
fi
