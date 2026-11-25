#!/bin/sh
# Writes <dir>/seed.iso, the first boot's cidata disk, as 2-main-runs.md specifies it.
# seed.sh <dir> <ssh public key> <instance name>. Needs genisoimage, xorriso or mkisofs.
set -eu
d=$1; key=$2; name=$3; s=$(mktemp -d)
cat > "$s/user-data" <<EOT
#cloud-config
users:
  - name: kit
    shell: /bin/bash
    lock_passwd: true
    sudo: "ALL=(ALL) NOPASSWD:ALL"
    ssh_authorized_keys: ["$key"]
ssh_pwauth: false
disable_root: true
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
printf 'instance-id: %s-%s\nlocal-hostname: main\n' "$name" "$(od -An -N8 -tx8 /dev/urandom | tr -d ' ')" > "$s/meta-data"
for m in genisoimage mkisofs; do
  command -v $m >/dev/null && { $m -quiet -output "$d/seed.iso" -volid cidata -joliet -rock "$s/user-data" "$s/meta-data"; rm -rf "$s"; exit 0; }
done
command -v xorriso >/dev/null && { xorriso -as mkisofs -quiet -output "$d/seed.iso" -volid cidata -joliet -rock "$s/user-data" "$s/meta-data"; rm -rf "$s"; exit 0; }
echo "seed.sh: needs genisoimage, mkisofs or xorriso" >&2; exit 1
