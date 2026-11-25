#!/bin/bash
# The Linux analogue of test 35: Silo's gvproxy, built from gvisor-tap-vsock 197eea05 with
# net/gvproxy-silo.patch (#380), a network namespace for the computer, an OpenSSH server in it.
# Sealed, root in the namespace must reach nothing of this machine, by TCP or ping, and still the
# internet (through this machine's network) and Silo's SSH forward; closed, nothing at all, and
# SSH still. Unprivileged ICMP is allowed for the run, as a Mac allows it, so pings are real. Skips (exit 0) without root, namespaces, sshd, git or
# a network.
set -u
[ "$(uname -s)" = Linux ] || { echo "Linux only: skipped"; exit 0; }
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
tmp="$(mktemp -d /tmp/kit35.XXXXXX)"
pgr="$(sysctl -n net.ipv4.ping_group_range 2>/dev/null | tr '\t' ' ')"
sysctl -qw net.ipv4.ping_group_range="0 2147483647" 2>/dev/null
trap 'rm -rf "$tmp"; [ -n "$pgr" ] && sysctl -qw net.ipv4.ping_group_range="$pgr"' EXIT
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
# This machine's router sits on TEST-NET (192.0.2.0/24), which is not a private range: #380's
# host-networks entry must refuse it as one of the networks this machine is on.
if ! grep -q '"key":"sealed-holds"' "$KIT_OUT"; then
  echo "FAIL: expected sealed-holds"; grep -E '"kind":"(outcome|fact)"' "$KIT_OUT" | tail -25; exit 1
fi
echo "ok   35: sealed-holds"

# The planted fault: gvproxy as released, standing in for Silo's build (-sealed and -closed
# dropped). The test must see the Mac reached.
m="$tmp/mutant"; mkdir -p "$m/bin"
( . "$KIT_ROOT/lib/common.sh"; . "$KIT_ROOT/lib/net.sh"; net_gvproxy_get ) || { echo "skip the planted fault: gvproxy as released could not be downloaded"; exit 0; }
cp "$KIT_WORK_ROOT/bin/gvproxy-v0.9.0" "$m/bin/"
cat > "$m/bin/gvproxy-197eea05-silo" <<'EOS'
#!/bin/sh
# a planted fault: gvproxy as released, claiming to be Silo's build
case "$1" in -help) echo "  -sealed"; echo "  -closed"; exit 0 ;; -version) exec "$(dirname "$0")/gvproxy-v0.9.0" -version ;; esac
exec "$(dirname "$0")/gvproxy-v0.9.0" -config "$2"
EOS
chmod +x "$m/bin/gvproxy-197eea05-silo"
( . "$KIT_ROOT/lib/common.sh"; . "$KIT_ROOT/lib/net.sh"; net_sha256 "$KIT_ROOT/net/gvproxy-silo.patch" ) > "$m/bin/gvproxy-197eea05-silo.patch-sha256"
: > "$KIT_OUT"; rm -rf "$KIT_WORK"; mkdir -p "$KIT_WORK"
KIT_WORK_ROOT="$m" bash "$KIT_ROOT/tests/35-sealed-gvproxy.sh" < /dev/null > "$tmp/mutant.log" 2>&1
if grep -q '"key":"sealed-leaks"' "$KIT_OUT"; then
  echo "ok   35 with gvproxy as released in the sealed build's place: sealed-leaks ($(grep '"kind":"outcome"' "$KIT_OUT" | sed 's/.*"text":"//' | cut -c1-120))"
else
  echo "FAIL: the planted fault was not caught"; tail -20 "$tmp/mutant.log"; exit 1
fi
