# shellcheck shell=bash
# Helpers for tests 47 to 49: option B's Linux main on the Mac, through libkrun (#381). Sourced
# after lib/common.sh and lib/vm.sh. Written for macOS /bin/bash 3.2.
#
# #381's kit (research/2026-11-25-linux-main/kit/) runs these questions on a Linux host with KVM;
# these are the ones only the Mac answers (its L1 to L8). Differences from #381:
# - Ubuntu's own kernel and initrd, from the same release's "unpacked" folder: a Mac cannot build
#   Silo's kernel. KIT_KRUN_KERNEL (and KIT_KRUN_INITRD) take one built by #381's kernel/build.sh.
# - libkrun from Homebrew (brew tap slp/krun; brew install libkrun), krun/krun-boot.c built by
#   ./kit build and signed with the hypervisor entitlement.
# - The seed disk is made with hdiutil; there is no network device (SSH is over vsock).
#
# Everything is under KRUN_HOME (~/SiloKit/linuxmain): image/disk.raw, image/Image, image/initrd,
# and one folder per computer under vms/: disk.raw (a clonefile of the image), seed.iso,
# ssh.sock, console.log, krun.pid.

KRUN_HOME="${KIT_WORK_ROOT:-$HOME/SiloKit}/linuxmain"
KRUN_BOOT="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin/krun-boot"
KRUN_URL="${KIT_KRUN_IMAGE_URL:-https://cloud-images.ubuntu.com/releases/26.04/release}"
KRUN_IMG="ubuntu-26.04-server-cloudimg-arm64.img"
KRUN_VMLINUZ="ubuntu-26.04-server-cloudimg-arm64-vmlinuz-generic"
KRUN_INITRD="ubuntu-26.04-server-cloudimg-arm64-initrd-generic"

krun_ready() { [ -f "$KRUN_HOME/image/disk.raw" ] && [ -f "$KRUN_HOME/image/Image" ]; }

# krun_need: skip the test unless the Mac can run it; prepare the image after asking.
krun_need() {
  kit_need_mac
  [ -x "$KRUN_BOOT" ] || kit_skip "krun-boot is not built: brew tap slp/krun && brew install libkrun, then ./kit build"
  krun_ready && return 0
  kit_need_free_gb 8
  kit_confirm "Download Ubuntu 26.04's arm64 cloud image (about 950 MB), its kernel and initrd (about 100 MB) from cloud-images.ubuntu.com, check them against Ubuntu's SHA256SUMS, and convert the image to a raw disk (about 4 GB, sparse) in $KRUN_HOME/image."
  krun_prepare > "$KIT_WORK/prepare.log" 2>&1 || { kit_attach "$KIT_WORK/prepare.log"; kit_fail "the Linux image could not be prepared: $(tail -2 "$KIT_WORK/prepare.log" | tr '\n' ' ')"; }
}

_krun_get() { # URL SUMS NAME: download into the current folder and check against SUMS
  [ -f "$3" ] || curl -fSL --retry 2 -o "$3.part" "$1/$3" && mv "$3.part" "$3" 2>/dev/null
  [ -f "$3" ] || return 1
  grep " \*\{0,1\}$3\$" "$2" | awk '{print $1}' > "$3.want"
  [ "$(python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$3")" = "$(cat "$3.want")" ] ||
    { echo "$3: sha256 does not match $2"; rm -f "$3"; return 1; }
}

krun_prepare() {
  mkdir -p "$KRUN_HOME/image" && cd "$KRUN_HOME/image" || return 1
  curl -fsSL -o SHA256SUMS "$KRUN_URL/SHA256SUMS" && curl -fsSL -o SHA256SUMS.unpacked "$KRUN_URL/unpacked/SHA256SUMS" || return 1
  _krun_get "$KRUN_URL" SHA256SUMS "$KRUN_IMG" || return 1
  python3 -I "$KIT_ROOT/py/linuxmain/qcow2raw.py" "$KRUN_IMG" disk.raw.part && mv disk.raw.part disk.raw && rm -f "$KRUN_IMG" || return 1
  if [ -n "${KIT_KRUN_KERNEL:-}" ]; then
    cp "$KIT_KRUN_KERNEL" Image && { [ -z "${KIT_KRUN_INITRD:-}" ] || cp "$KIT_KRUN_INITRD" initrd; }
  else
    _krun_get "$KRUN_URL/unpacked" SHA256SUMS.unpacked "$KRUN_VMLINUZ" && _krun_get "$KRUN_URL/unpacked" SHA256SUMS.unpacked "$KRUN_INITRD" || return 1
    python3 -I "$KIT_ROOT/py/linuxmain/kernel.py" "$KRUN_VMLINUZ" Image && mv "$KRUN_INITRD" initrd && rm -f "$KRUN_VMLINUZ"
  fi
}

# krun_seed DIR NAME: DIR/seed.iso, #381's seed (user kit with the kit's key, sshd on vsock port 22,
# the reboot key powers off, cloud-init off after the first boot).
krun_seed() {
  local d="$1" s; s="$(mktemp -d)"
  cat > "$s/user-data" <<EOT
#cloud-config
users:
  - name: kit
    shell: /bin/bash
    lock_passwd: true
    sudo: "ALL=(ALL) NOPASSWD:ALL"
    ssh_authorized_keys: ["$(cat "$(_vm_key).pub")"]
ssh_pwauth: false
growpart: {mode: auto, devices: ["/"]}
write_files:
  - path: /etc/systemd/system/sshd-vsock.socket
    content: |
      [Socket]
      ListenStream=vsock::22
      Accept=yes
      [Install]
      WantedBy=sockets.target
  - path: /etc/systemd/system/sshd-vsock@.service
    content: |
      [Service]
      ExecStart=-/usr/sbin/sshd -i
      StandardInput=socket
  - path: /etc/systemd/logind.conf.d/silo.conf
    content: |
      [Login]
      HandleRebootKey=poweroff
runcmd:
  - [systemctl, enable, --now, sshd-vsock.socket]
  - [touch, /etc/cloud/cloud-init.disabled]
EOT
  printf 'instance-id: %s-%s\nlocal-hostname: main\n' "$2" "$(od -An -N8 -tx8 /dev/urandom | tr -d ' ')" > "$s/meta-data"
  rm -f "$d/seed.iso"
  if command -v hdiutil >/dev/null 2>&1; then
    hdiutil makehybrid -quiet -o "$d/seed.iso" -iso -joliet -default-volume-name cidata "$s"
  elif command -v genisoimage >/dev/null 2>&1; then
    genisoimage -quiet -output "$d/seed.iso" -volid cidata -joliet -rock "$s/user-data" "$s/meta-data"
  else
    xorriso -as mkisofs -quiet -output "$d/seed.iso" -volid cidata -joliet -rock "$s/user-data" "$s/meta-data"
  fi
  local rc=$?; rm -rf "$s"; return $rc
}

# krun_new NAME [GB]: a fresh computer: a clone of the image grown to GB (32), and a seed.
krun_new() {
  local v="$KRUN_HOME/vms/$1"; rm -rf "$v"; mkdir -p "$v"
  cp -c "$KRUN_HOME/image/disk.raw" "$v/disk.raw" 2>/dev/null || cp "$KRUN_HOME/image/disk.raw" "$v/disk.raw"
  python3 -c 'import os,sys; os.truncate(sys.argv[1], int(sys.argv[2]) << 30)' "$v/disk.raw" "${2:-32}"
  krun_seed "$v" "$1"
}

# krun_boot NAME [--no-seed]: start it in the background (no network device).
krun_boot() {
  local v="$KRUN_HOME/vms/$1" seed="" init=""; shift
  [ "${1:-}" = --no-seed ] || seed="--seed $v/seed.iso"
  [ -f "$KRUN_HOME/image/initrd" ] && init="--initrd $KRUN_HOME/image/initrd"
  rm -f "$v/ssh.sock"
  # shellcheck disable=SC2086
  "$KRUN_BOOT" --kernel "$KRUN_HOME/image/Image" $init --disk "$v/disk.raw" $seed --ssh-sock "$v/ssh.sock" \
    --console "$v/console.log" --cpus "${KIT_KRUN_CPUS:-4}" --mem "${KIT_KRUN_MEM:-4096}" > "$v/krun.log" 2>&1 &
  echo $! > "$v/krun.pid"
}
krun_pid() { cat "$KRUN_HOME/vms/$1/krun.pid" 2>/dev/null; }
krun_running() { local p; p="$(krun_pid "$1")"; [ -n "$p" ] && kill -0 "$p" 2>/dev/null; }

# krun_ssh NAME cmd...: over vsock, never a network.
krun_ssh() {
  local v="$KRUN_HOME/vms/$1"; shift
  ssh -F /dev/null -q -i "$(_vm_key)" -o IdentitiesOnly=yes -o BatchMode=yes -o StrictHostKeyChecking=no \
    -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 -o ProxyCommand="nc -U $v/ssh.sock" kit@main "$@" < /dev/null
}

# krun_wait_ssh NAME SECONDS: prints the seconds it took; 1 when it never answered.
krun_wait_ssh() {
  local t0; t0="$(kit_now)"
  while python3 -c "import sys,time; sys.exit(0 if time.time() - $t0 < $2 else 1)"; do
    krun_running "$1" || return 1
    krun_ssh "$1" true 2>/dev/null && { python3 -c "print('%.1f' % ($(kit_now) - $t0))"; return 0; }
    sleep 1
  done
  return 1
}

# krun_term NAME SECONDS: the stop request (SIGTERM, which krun-boot passes to libkrun's
# shutdown eventfd); prints the seconds until the process ended, or "none" when it did not.
krun_term() {
  local p t0; p="$(krun_pid "$1")"; t0="$(kit_now)"
  kill -TERM "$p" 2>/dev/null
  while kill -0 "$p" 2>/dev/null; do
    python3 -c "import sys,time; sys.exit(0 if time.time() - $t0 < $2 else 1)" || { echo none; return 1; }
    sleep 0.5
  done
  python3 -c "print('%.1f' % ($(kit_now) - $t0))"
}

# krun_kill NAME: a power cut.
krun_kill() { local p; p="$(krun_pid "$1")"; [ -n "$p" ] && kill -9 "$p" 2>/dev/null; sleep 1; return 0; }

krun_destroy() { krun_kill "$1"; [ "${KIT_KEEP_VMS:-0}" = 1 ] || rm -rf "${KRUN_HOME:?}/vms/$1"; }
