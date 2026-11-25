#!/bin/bash
# KSM on a shared host: does it merge pages of two members (two Unix users), and can member B
# learn which page member A holds? Turns KSM on fast for the test, then puts it back.
# $1 = the secret, 0..255. Run as root.
H=$(cd $(dirname $0); pwd); B=$(mktemp -d); gcc -O1 -o $B/ksmt $H/ksm.c; chmod 755 $B $B/ksmt
K=/sys/kernel/mm/ksm; old_run=$(cat $K/run); old_sleep=$(cat $K/sleep_millisecs); old_pages=$(cat $K/pages_to_scan)
echo 1 > $K/run; echo 10 > $K/sleep_millisecs; echo 1000 > $K/pages_to_scan
for u in silo-a silo-b; do id $u >/dev/null 2>&1 || useradd -M -s /usr/sbin/nologin $u; done
for secret in ${1:-173} none; do
  if [ $secret != none ]; then setpriv --reuid=silo-a --regid=silo-a --init-groups $B/ksmt victim $secret > $B/pid & sleep 1; fi
  echo "A holds page $secret; pages shared before B: $(cat $K/pages_shared)"
  echo "  B: $(setpriv --reuid=silo-b --regid=silo-b --init-groups $B/ksmt probe 5)"
  [ $secret != none ] && kill $(cat $B/pid)
done
echo $old_run > $K/run; echo $old_sleep > $K/sleep_millisecs; echo $old_pages > $K/pages_to_scan; rm -rf $B
