#!/bin/sh
# id: K15
# title: The probe's means exist in Silo's kernel, and what root can do to them
# needs: K01
# decides: processes-from-inside-linux, 7-processes.md
# question: Do tracepoint eBPF programs load with BTF (bpftrace as a stand-in for the probe); does the process connector give events; how long does a parallel build take with exec tracing on, against off?
TEST=K15; . "$(dirname "$0")/../lib.sh"; need_kvm
vm_new k15; vm_boot k15; vm_wait_ssh k15 300 >/dev/null || fail "no SSH"
vm_ssh k15 'sudo apt-get -qq install -y bpftrace build-essential git >/dev/null 2>&1; git clone -q --depth 1 https://github.com/redis/redis.git r' >/dev/null 2>&1
btf=$(vm_ssh k15 'test -f /sys/kernel/btf/vmlinux && echo yes || echo no')
ev=$(vm_ssh k15 'sudo timeout 5 bpftrace -e "tracepoint:sched:sched_process_exec { @n = count(); } interval:s:3 { exit(); }" -c "ls" 2>&1 | grep -o "@n: [0-9]*"')
off=$(vm_ssh k15 'cd r && make distclean >/dev/null 2>&1; s=$(date +%s.%N); make -j4 >/dev/null 2>&1; echo "$(date +%s.%N) - $s" | bc')
on=$(vm_ssh k15 'cd r && make distclean >/dev/null 2>&1; sudo bpftrace -e "tracepoint:sched:sched_process_exec,tracepoint:sched:sched_process_exit { @[probe] = count(); }" >/tmp/bt.txt 2>&1 & sleep 3; s=$(date +%s.%N); make -j4 >/dev/null 2>&1; e=$(date +%s.%N); sudo pkill bpftrace; echo "$e - $s" | bc')
vm_stop k15 >/dev/null
result K15 measured "BTF $btf; exec tracepoint $ev; redis build ${off}s without tracing, ${on}s with exec and exit traced (target: at most 3 % longer)"
