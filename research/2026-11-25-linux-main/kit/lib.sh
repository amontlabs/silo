# Shared by the kit's tests. Sourced, never run.
# Everything lives under $KIT (default ~/silo-kit); each test writes into $KIT/results.jsonl.
KIT=${KIT:-$HOME/silo-kit}
KIT_SRC=$(cd "$(dirname "${0}")/.." 2>/dev/null && pwd)
IMAGE_URL=${KIT_IMAGE_URL:-https://cloud-images.ubuntu.com/releases/26.04/release}
mkdir -p "$KIT/run" "$KIT/vms"

result() {  # result <id> <outcome> <note>
  printf '{"test":"%s","outcome":"%s","note":"%s","time":"%s"}\n' "$1" "$2" "$3" "$(date -u +%FT%TZ)" >> "$KIT/results.jsonl"
  echo "$1: $2 · $3"
}
skip() { result "$TEST" skipped "$1"; exit 0; }
fail() { result "$TEST" failed "$1"; exit 1; }
need() { for t in "$@"; do command -v "$t" >/dev/null 2>&1 || skip "needs $t"; done; }
need_kvm() { [ -c /dev/kvm ] && [ -w /dev/kvm ] || skip "needs /dev/kvm: this host has no KVM"; }
arch() { case $(uname -m) in aarch64) echo arm64 ;; x86_64) echo amd64 ;; *) uname -m ;; esac; }

# A computer of the kit: its folder holds disk.raw, seed.iso, ssh keys, sockets, console.log.
vm_new() {  # vm_new <name> [size]: a fresh disk from the image, and a seed
  v="$KIT/vms/$1"; rm -rf "$v"; mkdir -p "$v"
  cp --reflink=auto --sparse=always "$KIT/image/disk.raw" "$v/disk.raw"
  truncate -s "${2:-32G}" "$v/disk.raw"
  ssh-keygen -q -t ed25519 -N '' -f "$v/id" -C "kit $1"
  sh "$KIT_SRC/seed.sh" "$v" "$(cat "$v/id.pub")" "$1"
}
vm_boot() {  # vm_boot <name> [--no-net] [--no-seed]: starts it in the background
  v="$KIT/vms/$1"; shift
  net="--net-sock $v/net.sock"; seed="--seed $v/seed.iso"
  for a in "$@"; do case $a in --no-net) net= ;; --no-seed) seed= ;; esac; done
  if [ -n "$net" ]; then
    rm -f "$v/net.sock"
    gvproxy -listen-vfkit "unixgram://$v/net.sock" -ssh-port -1 >"$v/gvproxy.log" 2>&1 &
    echo $! > "$v/gvproxy.pid"; sleep 1
  fi
  fmt=raw; [ "$(arch)" = amd64 ] && fmt=elf
  "$KIT/bin/krun-boot" --kernel "$KIT/kernel/image" --kernel-format $fmt --disk "$v/disk.raw" $seed \
    --ssh-sock "$v/ssh.sock" --probe-sock "$v/probe.sock" $net --console "$v/console.log" \
    --cpus "${KIT_CPUS:-4}" --mem "${KIT_MEM:-4096}" >"$v/krun.log" 2>&1 &
  echo $! > "$v/krun.pid"
}
vm_ssh() {  # vm_ssh <name> <command...>: over vsock, never the network
  v="$KIT/vms/$1"; shift
  ssh -q -i "$v/id" -o IdentitiesOnly=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
      -o ProxyCommand="socat - UNIX-CONNECT:$v/ssh.sock" -o ConnectTimeout=5 kit@main "$@"
}
vm_wait_ssh() {  # vm_wait_ssh <name> <seconds>: prints how long it took, or fails
  start=$(date +%s.%N); end=$(( $(date +%s) + $2 ))
  while [ "$(date +%s)" -lt "$end" ]; do
    vm_ssh "$1" true 2>/dev/null && { echo "$(date +%s.%N) - $start" | bc; return 0; }
    sleep 0.5
  done
  return 1
}
vm_kill() {  # vm_kill <name>: a power cut
  v="$KIT/vms/$1"
  for p in krun gvproxy; do [ -f "$v/$p.pid" ] && kill -9 "$(cat "$v/$p.pid")" 2>/dev/null; rm -f "$v/$p.pid"; done
  sleep 1
}
vm_stop() {  # vm_stop <name>: poweroff over SSH, a power cut after 30 s
  v="$KIT/vms/$1"; vm_ssh "$1" sudo systemctl poweroff 2>/dev/null
  i=0; while [ $i -lt 30 ] && kill -0 "$(cat "$v/krun.pid")" 2>/dev/null; do sleep 1; i=$((i+1)); done
  vm_kill "$1"; echo $i
}
