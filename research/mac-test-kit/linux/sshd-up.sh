#!/bin/bash
# linux/sshd-up.sh DIR LISTEN_ADDR PORT: a throwaway OpenSSH server for the Linux analogues of
# tests 07 and 08 (it stands in for the macOS computer's sshd). Writes a host key, a client key
# (DIR/id_ed25519) and its config into DIR, starts sshd in the foreground of a background job and
# prints its pid. Needs root (sshd's privilege separation) and openssh-server. Linux only.
set -u
d="$1" addr="$2" port="$3"
SSHD="$(command -v sshd || echo /usr/sbin/sshd)"
[ -x "$SSHD" ] || { echo "no sshd: apt-get install openssh-server" >&2; exit 3; }
mkdir -p "$d" /run/sshd
chmod 700 "$d"
[ -f "$d/host_ed25519" ] || ssh-keygen -q -t ed25519 -N '' -f "$d/host_ed25519"
[ -f "$d/id_ed25519" ] || ssh-keygen -q -t ed25519 -N '' -C silo-kit-linux -f "$d/id_ed25519"
cp "$d/id_ed25519.pub" "$d/authorized_keys"
chmod 600 "$d/authorized_keys"
cat > "$d/sshd_config" <<EOF
ListenAddress $addr
Port $port
HostKey $d/host_ed25519
AuthorizedKeysFile $d/authorized_keys
PermitRootLogin prohibit-password
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
StrictModes no
PidFile $d/sshd.pid
Subsystem sftp internal-sftp
MaxStartups 100:30:200
EOF
"$SSHD" -D -e -f "$d/sshd_config" > "$d/sshd.log" 2>&1 &
echo $!
