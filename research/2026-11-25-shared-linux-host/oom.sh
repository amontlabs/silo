#!/bin/bash
# When the host runs out of memory, whose computer is stopped? A parent control group of 8 GiB
# plays the host. Member B runs one computer of 4 GiB. Member A starts three computers that grow
# to 1.5 GiB each, so the host runs out because of A.
# $1 = shared: both members in the host's group only. $1 = per-member: A's computers also in a
# group of A's own, limited to 3.5 GiB. Prints which computers are still running. cgroup v1. Root.
H=$(cd $(dirname $0); pwd); C=/sys/fs/cgroup/memory/silohost
cleanup() { for g in $C/a $C/b $C; do [ -d $g ] && { for p in $(cat $g/tasks); do kill -9 $p 2>/dev/null; done; sleep 0.5; rmdir $g; }; done; }
cleanup 2>/dev/null; mkdir -p $C; echo $((8<<30)) > $C/memory.limit_in_bytes
mkdir $C/a $C/b; [ "$1" = per-member ] && echo $((3584<<20)) > $C/a/memory.limit_in_bytes
run() { sh -c "echo \$\$ > $C/$1/cgroup.procs; exec python3 $H/hog.py $2 ${3:-0.05}" >/dev/null 2>&1 & echo $!; }
b1=$(run b 4096); sleep 6
a1=$(run a 1536 0.1); a2=$(run a 1536 0.1); a3=$(run a 1536 0.1); sleep 15
for x in "B's computer:$b1" "A's computer 1:$a1" "A's computer 2:$a2" "A's computer 3:$a3"; do
  p=${x#*:}; kill -0 $p 2>/dev/null && s=running || s=stopped; echo "${x%%:*} $s"
done
cleanup
