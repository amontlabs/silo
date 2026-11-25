#!/bin/sh
# id: K11
# title: File facts of a night of work, from checkpoints, against the kernel's own view
# needs: K07
# decides: file-changes-from-outside, 5-the-record.md
# question: With a checkpoint every 5 minutes during scripted agent-like work (clone, build, test, delete outside the repository), does the reader's diff of each pair equal the diff of the same two checkpoints mounted by the kernel after e2fsck's replay?
TEST=K11; . "$(dirname "$0")/../lib.sh"; need_kvm; need e2fsck losetup
vm_new k11; vm_boot k11; vm_wait_ssh k11 300 >/dev/null || fail "no SSH"
vm_ssh k11 'sudo apt-get -qq install -y git build-essential >/dev/null 2>&1; nohup sh -c "
  git clone -q https://github.com/redis/redis.git work/redis && cd work/redis && make -j4 >/dev/null 2>&1;
  mkdir -p ~/shared && cp -r src ~/shared/; sleep 60; rm -rf ~/shared; make distclean >/dev/null 2>&1; make -j4 >/dev/null 2>&1
" >/dev/null 2>&1 &'
n=${KIT_K11_PAIRS:-12}; bad=0
cp --reflink=always "$KIT/vms/k11/disk.raw" "$KIT/vms/k11/p0.raw" || skip "no reflink under $KIT"
for k in $(seq 1 "$n"); do
  sleep "${KIT_K11_INTERVAL:-300}"
  cp --reflink=always "$KIT/vms/k11/disk.raw" "$KIT/vms/k11/p$k.raw"
  r=$(sh "$KIT_SRC/tools/compare-pair.sh" "$KIT/vms/k11/p$((k-1)).raw" "$KIT/vms/k11/p$k.raw")
  echo "pair $k: $r" >> "$KIT/vms/k11/report.txt"
  case $r in equal*) ;; *) bad=$((bad+1)) ;; esac
  rm -f "$KIT/vms/k11/p$((k-1)).raw"
done
vm_stop k11 >/dev/null
[ $bad = 0 ] && result K11 passed "$n pairs, the reader's diff equal to the kernel's for each (vms/k11/report.txt)" || result K11 failed "$bad of $n pairs differ; vms/k11/report.txt"
