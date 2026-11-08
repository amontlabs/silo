#!/usr/bin/env bash
# Prepares a GitHub ubuntu-24.04 runner (or any Ubuntu 24.04 x86_64 host with passwordless sudo) for run.sh:
# packages, the Landlock init shim, the unpacked Silo guest image, cgroup delegation, the host-side proxy and the
# host's nftables table. Every change to the host's own settings is printed, because each is a cost of the design.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
LOCK=$REPO/app/SiloUI/guest-image/image-lock.json

say "workspace $WORK ($(df -T "$(dirname "$WORK")" | awk 'NR==2{print $2}'))"
sudo mkdir -p "$WORK"
sudo chown "$(id -u):$(id -g)" "$WORK"

say "packages"
sudo apt-get update -qq
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
  crun podman uidmap nftables tinyproxy gcc libc6-dev linux-libc-dev python3 curl jq netcat-openbsd iproute2 util-linux

say "host settings this design needs"
# Ubuntu 24.04 refuses unprivileged user namespaces to unconfined programs. Rootless crun (and the outer
# namespace of userns.py) need them. The alternative is an AppArmor profile with a `userns,` rule per program.
echo "kernel.apparmor_restrict_unprivileged_userns before: $(sysctl -n kernel.apparmor_restrict_unprivileged_userns 2>&1)"
sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0 || true
echo "kernel.apparmor_restrict_unprivileged_unconfined before: $(sysctl -n kernel.apparmor_restrict_unprivileged_unconfined 2>&1)"
sudo sysctl -w kernel.apparmor_restrict_unprivileged_unconfined=0 || true
echo "user.max_user_namespaces: $(sysctl -n user.max_user_namespaces)"
me=$(id -un)
if ! grep -q "^$me:" /etc/subuid; then sudo usermod --add-subuids 100000-165535 --add-subgids 100000-165535 "$me"; echo "added sub-ids for $me"; fi
echo "subuid: $(grep "^$me:" /etc/subuid)  subgid: $(grep "^$me:" /etc/subgid)"

say "the Landlock init shim and the boot layer"
rm -rf "$BOOT"
mkdir -p "$BOOT/silo" "$BOOT/workspace"
gcc -O2 -Wall -static -o "$BOOT/silo/init" "$C1/silo-init.c"
cp "$C1"/boot/* "$BOOT/silo/"
chmod 755 "$BOOT"/silo/*
"$BOOT/silo/init" --probe || true

say "the Silo guest image (x86_64, pinned in image-lock.json)"
read -r REF DIGEST ARCHIVE_SHA RELEASE < <(python3 - "$LOCK" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
i = d['images']['amd64']
print(i['imageReference'], i['imageDigest'], i['archiveSha256'], d['releaseUrl'])
PY
)
PODMAN=(sudo podman --root "$WORK/podman" --runroot "$WORK/podman-run")
if "${PODMAN[@]}" pull -q "docker://$REF" > "$RAW/podman-pull.txt" 2>&1; then
  echo "pulled $REF"
else
  echo "pull failed: $(tail -n 3 "$RAW/podman-pull.txt"); falling back to the release archive"
  curl -fsSL -o "$WORK/image-amd64.tar.gz" "$RELEASE/image-amd64.tar.gz"
  echo "$ARCHIVE_SHA  $WORK/image-amd64.tar.gz" | sha256sum -c -
  "${PODMAN[@]}" load -i "$WORK/image-amd64.tar.gz"
fi
IMAGE_ID=$("${PODMAN[@]}" images --no-trunc --format '{{.ID}}' | head -1)
echo "pinned imageDigest: $DIGEST"
echo "pulled image id:    sha256:${IMAGE_ID#sha256:}"
if [ "sha256:${IMAGE_ID#sha256:}" != "$DIGEST" ]; then echo "WARNING: the pulled image is not the pinned image" >&2; fi
rm -rf "$WORK/base"; mkdir -p "$BASE"
CID=$("${PODMAN[@]}" create "$IMAGE_ID" true)
# Extracted in the outer user namespace, so a file the image gives to uid 33 is owned by sub-id 33 on disk.
"${PODMAN[@]}" export "$CID" | in_userns tar -xpf - -C "$BASE" --numeric-owner
"${PODMAN[@]}" rm "$CID" >/dev/null
echo "unpacked rootfs: $(sudo du -sm "$BASE" | cut -f1) MiB; passwd lines for uid 1000: $(grep ':1000:' "$BASE/etc/passwd")"
echo "image files: $(cat "$BASE/usr/local/share/silo/guest-image.json" 2>/dev/null)"
"${PODMAN[@]}" system prune -af >/dev/null 2>&1 || true

say "cgroup v2 delegation (what a systemd Delegate=yes slice gives the user)"
echo "controllers at the root: $(cat /sys/fs/cgroup/cgroup.controllers)"
echo "+cpu +memory +pids" | sudo tee /sys/fs/cgroup/cgroup.subtree_control >/dev/null || true
sudo mkdir -p "$CGROOT"
echo "+cpu +memory +pids" | sudo tee "$CGROOT/cgroup.subtree_control" >/dev/null
sudo chown -R "$(id -u):$(id -g)" "$CGROOT"
echo "delegated $CGROOT: $(cat "$CGROOT/cgroup.controllers") -> $(cat "$CGROOT/cgroup.subtree_control")"

say "host nftables: veth traffic may reach only the proxy port, and is never forwarded"
sudo nft delete table inet silo_c1 2>/dev/null || true
sudo nft -f - <<'NFT'
table inet silo_c1 {
  chain input {
    type filter hook input priority -10; policy accept;
    iifname "vh*" tcp dport 3128 accept
    iifname "vh*" counter drop
  }
  chain forward {
    type filter hook forward priority -10; policy accept;
    iifname "vh*" counter drop
    oifname "vh*" counter drop
  }
}
NFT
echo "ip_forward: $(sysctl -n net.ipv4.ip_forward)"

say "the host-side HTTP(S) proxies (proxy.sh starts one on port 3128 at a time; squid is the one the workloads use)"
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends squid
sudo systemctl disable --now tinyproxy squid >/dev/null 2>&1 || true
bash "$C1/proxy.sh" squid

say "a canary service on the host that a container must not reach"
mkdir -p "$WORK/canary"
( cd "$WORK/canary" && echo canary > index.html && setsid nohup python3 -m http.server 8000 --bind 0.0.0.0 </dev/null > "$WORK/canary.log" 2>&1 & )
sleep 1
echo "canary: $(curl -s --max-time 3 http://127.0.0.1:8000/)"
say "setup done"
