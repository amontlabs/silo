# shellcheck shell=bash
# Shared helpers for setup.sh and run.sh (phase C1, docs/forks-ci/PLAN.md).
C1=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$C1/../../.." && pwd)
WORK=${C1_WORK:-/mnt/c1}
OUT=${C1_OUT:-$WORK/evidence}
RAW=$OUT/raw
BASE=$WORK/base/rootfs
BOOT=$WORK/boot
CTR=$WORK/ctr
CGROOT=/sys/fs/cgroup/silo-c1
[ -d "$WORK" ] || { sudo mkdir -p "$WORK" && sudo chown "$(id -u):$(id -g)" "$WORK"; }
mkdir -p "$RAW" "$CTR"

now() { date +%s.%N; }
sub() { awk -v a="$1" -v b="$2" 'BEGIN{printf "%.2f", b-a}'; }
say() { echo "[$(date +%H:%M:%S)] $*"; }
stats() { python3 "$REPO/docs/forks-ci/checks/measure_stats.py" "$@"; }
result() { printf '%s\t%s\n' "$1" "$2" >> "$OUT/results.tsv"; }

# wait_marker FILE MARKER SECONDS: a line starting with MARKER appears in FILE (or the container exits first).
wait_marker() {
  local file=$1 marker=$2 limit=$3 t; t=$(now)
  while ! grep -Eq "^$marker( |\$)" "$file" 2>/dev/null; do
    [ -e "$(dirname "$file")/exit" ] && { grep -Eq "^$marker( |\$)" "$file" 2>/dev/null && return 0; return 1; }
    awk -v a="$t" -v b="$(now)" -v l="$limit" 'BEGIN{exit !(b-a>l)}' && return 2
    sleep 0.02
  done
}
marker_ts() { awk -v m="$2" '$1==m{print $NF; exit}' "$1"; }

# net_setup IDX PID: a veth pair into the container's network namespace, addresses, and an nftables policy inside
# it: output and input default-deny, output allowed only to the host end's proxy port. Runs as root (the one
# privileged step; the container itself never has a capability over its network namespace).
net_setup() {
  sudo bash -s -- "$1" "$2" <<'EOS'
set -e
idx=$1; pid=$2
h=vh$idx; c=vc$idx; hip=10.200.$idx.1; cip=10.200.$idx.2
ip link del "$h" 2>/dev/null || true
ip link add "$h" type veth peer name "$c"
ip link set "$c" netns "$pid"
ip addr add "$hip/30" dev "$h"
ip link set "$h" up
nsenter -t "$pid" -n ip link set lo up
nsenter -t "$pid" -n ip link set "$c" name eth0
nsenter -t "$pid" -n ip addr add "$cip/30" dev eth0
nsenter -t "$pid" -n ip link set eth0 up
nsenter -t "$pid" -n nft -f - <<NFT
table inet silo {
  chain input {
    type filter hook input priority 0; policy drop;
    iifname "lo" accept
    ct state established,related accept
    counter reject
  }
  chain output {
    type filter hook output priority 0; policy drop;
    oifname "lo" accept
    ct state established,related accept
    ip daddr $hip tcp dport 3128 accept
    counter reject
  }
}
NFT
nsenter -t "$pid" -n ip route add default via "$hip"
EOS
}

# start_ctr NAME MODE: starts a container and returns when its shell is ready (READY printed).
# Options as variables: CTR_UID (0), CTR_CAPS (none|relaxed), CTR_NET (1.. = network index; 0 = none),
# CTR_HOLD (1 = stay alive after the mode), CTR_CONNECT / CTR_BIND (Landlock TCP ports), CTR_ENV (array of K=V).
# Writes $CTR/NAME/{out,err,exit,pid,t0,timing}; timing is "UP_SECONDS READY_SECONDS" since the start command.
start_ctr() {
  local name=$1 mode=$2 d=$CTR/$1 env_args=() kv
  mkdir -p "$d"/{work,merged,bundle,state}
  [ -d "$d/upper" ] || mkdir "$d/upper"
  rm -f "$d/exit" "$d/pid" "$d/out" "$d/err"
  [ "${CTR_HOLD:-0}" = 1 ] && env_args+=(--env C1_HOLD=1)
  for kv in ${CTR_ENV[@]+"${CTR_ENV[@]}"}; do env_args+=(--env "$kv"); done
  python3 "$C1/mkconfig.py" "$d/bundle/config.json" --name "$name" --rootfs "$d/merged" --cgroup "/silo-c1/$name" \
    --mode "$mode" --uid "${CTR_UID:-0}" --gid "${CTR_UID:-0}" --caps "${CTR_CAPS:-none}" --net "${CTR_NET:-0}" \
    --connect "${CTR_CONNECT:-3128}" --bind "${CTR_BIND:-}" ${env_args[@]+"${env_args[@]}"} || return 1
  local t0; t0=$(now); echo "$t0" > "$d/t0"
  ( python3 "$C1/userns.py" -- bash "$C1/inside.sh" "$d" "$name" "$BOOT" "$BASE" > "$d/out" 2> "$d/err"; echo $? > "$d/exit" ) &
  if [ "${CTR_NET:-0}" != 0 ]; then
    for _ in $(seq 1 1000); do [ -s "$d/pid" ] && break; [ -e "$d/exit" ] && break; sleep 0.01; done
    if [ ! -s "$d/pid" ]; then echo "$name: crun wrote no pid file (exit $(cat "$d/exit" 2>/dev/null)): $(tail -c 400 "$d/err")" >&2; return 1; fi
    echo "$CTR_NET" > "$d/netidx"
    net_setup "$CTR_NET" "$(cat "$d/pid")" > "$d/net.log" 2>&1 || { echo "$name: net_setup failed: $(tail -c 300 "$d/net.log")" >&2; return 1; }
  fi
  wait_marker "$d/out" READY 120
  local rc=$?
  if [ "$rc" != 0 ]; then echo "$name: no READY (rc $rc, exit $(cat "$d/exit" 2>/dev/null)): $(tail -c 600 "$d/err")" >&2; return 1; fi
  echo "$(sub "$t0" "$(marker_ts "$d/out" UP)") $(sub "$t0" "$(marker_ts "$d/out" READY)")" > "$d/timing"
}

# wait_exit NAME SECONDS: the container's crun has exited (its mode finished with C1_HOLD unset).
wait_exit() {
  local d=$CTR/$1 t; t=$(now)
  while [ ! -e "$d/exit" ]; do
    awk -v a="$t" -v b="$(now)" -v l="$2" 'BEGIN{exit !(b-a>l)}' && return 1
    sleep 0.1
  done
}

# stop_ctr NAME: ends a container (kills its init, which ends its PID namespace) and removes its host-side pieces.
stop_ctr() {
  local d=$CTR/$1
  if [ -s "$d/pid" ] && [ ! -e "$d/exit" ]; then sudo kill -KILL "$(cat "$d/pid")" 2>/dev/null; fi
  wait_exit "$1" 30
  [ -s "$d/netidx" ] && sudo ip link del "vh$(cat "$d/netidx")" 2>/dev/null
  sudo rmdir "$CGROOT/$1" 2>/dev/null
  return 0
}

# drop_ctr NAME: stop and delete everything of the container, including its upper layer.
drop_ctr() { stop_ctr "$1"; sudo rm -rf "$CTR/$1"; }

# pss_of NAME: PSS and cgroup figures of a running container.
pss_of() { sudo python3 "$C1/pss.py" "$CGROOT/$1"; }
pss_mib() { pss_of "$1" | awk '$1=="PSS_MIB"{print $2}'; }

# in_userns CMD...: run as root of the outer user namespace (to read or copy files owned by sub-ids).
in_userns() { python3 "$C1/userns.py" -- "$@"; }
