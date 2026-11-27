#!/bin/bash
# KSM off; Ana's three forks share her snapshot, Ben probes with his own. $1 = Ana's secret page.
# Prints the memory Ana's forks use (Rss each, Pss summed) and Ben's probe. Run as root.
H=$(cd $(dirname $0); pwd); B=$(mktemp -d); gcc -O1 -o $B/fs $H/fork-share.c; chmod 755 $B $B/fs
K=/sys/kernel/mm/ksm; echo "KSM run = $(cat $K/run) (0 is off)"
for u in silo-a silo-b; do id $u >/dev/null 2>&1 || useradd -M -s /usr/sbin/nologin $u; done
mkdir -m 700 $B/ana $B/ben; chown silo-a $B/ana; chown silo-b $B/ben
for secret in ${1:-173} 42; do
  setpriv --reuid=silo-a --regid=silo-a --init-groups $B/fs make $B/ana/snap $secret
  setpriv --reuid=silo-b --regid=silo-b --init-groups $B/fs make $B/ben/snap -1     # every page a guess "secret"
  P=""; for i in 1 2 3; do setpriv --reuid=silo-a --regid=silo-a --init-groups $B/fs fork $B/ana/snap > $B/p$i & done; sleep 1
  P=$(cat $B/p1 $B/p2 $B/p3)
  r=0; s=0; for p in $P; do r=$((r + $(awk '/^Rss:/{x+=$2} END{print x}' /proc/$p/smaps_rollup))); s=$((s + $(awk '/^Pss:/{x+=$2} END{print x}' /proc/$p/smaps_rollup))); done
  echo "Ana's secret page $secret; her 3 forks of a 1 MiB snapshot: Rss $r KiB in all, Pss $s KiB in all (process code included)"
  echo "  Ben: $(setpriv --reuid=silo-b --regid=silo-b --init-groups $B/fs probe $B/ben/snap)"
  kill $P; wait 2>/dev/null
done
rm -rf $B
