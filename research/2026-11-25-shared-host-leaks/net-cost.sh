#!/bin/bash
# What a link per computer, its listener and the host firewall cost, at N computers (default 200).
# Setup time per computer, kernel memory per link, the metadata server's memory, the time of one
# token request, and throughput (tp.py, three runs of 3 s) from a computer to the boundary with no firewall, with the one
# group-based table of net-fix.sh, and with a rule per link instead. Run as root; cleans up.
H=$(cd $(dirname $0); pwd); W=$(mktemp -d); N=${N:-200}
clean() { for n in $(ip netns list | awk '/^c[0-9]/{print $1}'); do ip netns del $n; done; nft delete table inet silo 2>/dev/null
  pkill -f meta2.py; pkill -f "tp.py server"; sleep 1; }
mk() { ip netns add c$1; ip link add h$1 group 7 type veth peer name e0 netns c$1
  ip addr add 169.254.169.254/32 dev h$1; sysctl -qw net.ipv4.conf.h$1.arp_announce=2; ip link set h$1 up
  ip netns exec c$1 ip link set lo up; ip netns exec c$1 ip addr add 10.77.$(( $1 / 250 )).$(( $1 % 250 + 2 ))/32 dev e0; ip netns exec c$1 ip link set e0 up
  ip netns exec c$1 ip route add 169.254.169.254 dev e0; ip route add 10.77.$(( $1 / 250 )).$(( $1 % 250 + 2 ))/32 dev h$1; echo "h$1 p$1" >> $W/links; }
slab() { awk '/^(Slab|Percpu|KernelStack|PageTables):/{s+=$2} END {print s}' /proc/meminfo; }
clean; : > $W/links; sync; echo 3 > /proc/sys/vm/drop_caches; k0=$(slab); t0=$(date +%s%N)
for i in $(seq 1 $N); do mk $i; done
t1=$(date +%s%N); k1=$(slab)
echo "$N computers made: $(( (t1-t0)/N/1000000 )) ms each (namespace, link, addresses, route); kernel memory $(( (k1-k0)/N )) KiB each"
python3 $H/meta2.py $W/links >/dev/null & sleep 2
echo "metadata server, $N listeners: $(( $(ps -o rss= -p $!) )) KiB resident, $(ls /proc/$!/fd | wc -l) open files"
echo "token request from computer 1, $N links, no firewall: $(ip netns exec c1 python3 $H/lat.py 2000)"
python3 $H/tp.py server 169.254.169.254 5201 & sleep 1
tp() { for r in 1 2 3; do ip netns exec c1 python3 $H/tp.py client 169.254.169.254 5201 3; done | tr '\n' ' '; }
echo "throughput computer 1 to the boundary, no firewall:        $(tp)"
nft -f - <<'NFT'
table inet silo {
  chain input { type filter hook input priority filter - 10; policy accept;
    iifgroup 7 ct state established,related accept
    iifgroup 7 ip daddr 169.254.169.254 tcp dport { 80, 5201 } accept
    iifgroup 7 counter drop
  }
  chain forward { type filter hook forward priority filter - 10; policy accept;
    iifgroup 7 counter drop
    oifgroup 7 counter drop
  }
}
NFT
echo "throughput, the group table (5 rules for every link):      $(tp)"
echo "token request, the group table:                             $(ip netns exec c1 python3 $H/lat.py 2000)"
nft delete table inet silo
{ echo 'table inet silo { chain input { type filter hook input priority filter - 10; policy accept;'
  for i in $(seq $N -1 1); do echo "iifname \"h$i\" ct state established,related accept"; echo "iifname \"h$i\" ip daddr 169.254.169.254 tcp dport { 80, 5201 } accept"; echo "iifname \"h$i\" drop"; done
  printf '}\n}\n'; } > $W/per-link.nft
nft -f $W/per-link.nft
echo "throughput, $((3*N)) rules, one set per link, computer 1's last: $(tp)"
echo "token request, rules per link:                              $(ip netns exec c1 python3 $H/lat.py 2000)"
clean; rm -rf $W
