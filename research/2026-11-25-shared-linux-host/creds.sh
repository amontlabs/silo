#!/bin/bash
# Who on a shared host can read a member's secret from that member's boundary process, and use
# that member's SSH agent. Members A and B are Unix users; root is the host's administrator.
# Run as root. Makes the users silo-a and silo-b if missing, and removes what it started.
H=$(cd $(dirname $0); pwd); T=$(mktemp -d); gcc -O1 -o $T/holder $H/holder.c; chmod 755 $T $T/holder
for u in silo-a silo-b; do id $u >/dev/null 2>&1 || useradd -M -s /usr/sbin/nologin $u; done
as() { u=$1; shift; setpriv --reuid=$u --regid=$u --init-groups "$@"; }
for dump in dumpable nodump; do
  as silo-a $T/holder sk-live-member-a-0001 $dump > $T/pid & sleep 0.5; P=$(cat $T/pid)
  echo "== A's boundary process ($dump)"
  echo "another process of A:  $(as silo-a python3 $H/reader.py $P sk-live-)"
  echo "a process of B:        $(as silo-b python3 $H/reader.py $P sk-live-)"
  echo "root:                  $(python3 $H/reader.py $P sk-live-)"
  kill $P; wait $P 2>/dev/null
done
echo "== A's SSH agent, its socket in a folder only A can open"
install -d -o silo-a -g silo-a -m 700 $T/a; as silo-a ssh-agent -a $T/a/agent.sock >/dev/null
as silo-a ssh-keygen -q -t ed25519 -N '' -f $T/a/key; as silo-a env SSH_AUTH_SOCK=$T/a/agent.sock ssh-add -q $T/a/key 2>/dev/null
try() { out=$("$@" 2>&1 | head -1); case "$out" in ssh-ed25519*) echo "listed A's key";; *) echo "refused ($out)";; esac; }
echo "a process of A:        $(try as silo-a env SSH_AUTH_SOCK=$T/a/agent.sock ssh-add -L)"
echo "a process of B:        $(try as silo-b env SSH_AUTH_SOCK=$T/a/agent.sock ssh-add -L)"
echo "root:                  $(try env SSH_AUTH_SOCK=$T/a/agent.sock ssh-add -L)"
pkill -u silo-a ssh-agent; rm -rf $T
