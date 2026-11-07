#!/bin/bash
# The developer workload timed in the container and in the microVM: the same script, as root, fresh disk each run.
# Prints "PHASE name seconds rc" per phase. Needs the network (through the proxy in a container, direct in a microVM).
export DEBIAN_FRONTEND=noninteractive
export HOME=${HOME:-/root}
mkdir -p /tmp/c1-logs /workspace
APT_OPTS=(-o APT::Sandbox::User=root)
if [ -n "${http_proxy:-}" ]; then APT_OPTS+=(-o "Acquire::http::Proxy=$http_proxy" -o "Acquire::https::Proxy=$http_proxy"); fi
now() { date +%s.%N; }
T_START=$(now)
trap 'echo "WORKLOAD_TOTAL $(awk -v a="$T_START" -v b="$(now)" '"'"'BEGIN{printf "%.2f", b-a}'"'"')"; echo WORKLOAD_END' EXIT
phase() { # NAME COMMAND...
  local name=$1; shift
  local t; t=$(now)
  "$@" >"/tmp/c1-logs/$name.log" 2>&1
  local rc=$?
  echo "PHASE $name $(awk -v a="$t" -v b="$(now)" 'BEGIN{printf "%.2f", b-a}') $rc"
  [ "$rc" = 0 ] || echo "PHASE_TAIL $name $(tail -n 4 "/tmp/c1-logs/$name.log" | tr '\n' ' ' | cut -c1-400)"
  return "$rc"
}
phase git_clone_express git clone --quiet https://github.com/expressjs/express.git /workspace/express
phase apt_update apt-get "${APT_OPTS[@]}" update
phase apt_download apt-get "${APT_OPTS[@]}" install -y -d --no-install-recommends build-essential nodejs npm || exit 1
phase apt_unpack_configure apt-get "${APT_OPTS[@]}" install -y --no-install-recommends build-essential nodejs npm || exit 1
phase npm_install bash -c 'cd /workspace/express && npm install --no-audit --no-fund --loglevel=error'
phase rustup_install bash -c 'curl -fsSL --proto "=https" https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path'
phase cargo_install_hexyl "$HOME/.cargo/bin/cargo" install --locked hexyl
phase git_clone_zlib git clone --quiet --depth 1 https://github.com/madler/zlib.git /workspace/zlib
phase make_zlib bash -c 'cd /workspace/zlib && ./configure && make -j2'
echo "WORKLOAD_DONE"
