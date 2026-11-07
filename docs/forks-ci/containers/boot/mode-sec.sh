#!/bin/bash
# Proves the baseline is in force from inside: capabilities, no_new_privs, seccomp, Landlock, masked paths.
echo "CHECK id $(id)"
for key in CapInh CapPrm CapEff CapBnd CapAmb NoNewPrivs Seccomp; do
  echo "CHECK status.$key $(awk -v k="$key:" '$1==k{print $2}' /proc/self/status)"
done
echo "CHECK uid_map $(tr -s ' ' < /proc/self/uid_map | tr '\n' ';')"
python3 /silo/sec_probe.py
echo "CHECK unshare-user $(unshare -U true 2>&1; echo rc=$?)"
echo "CHECK mount-tmpfs $(mount -t tmpfs none /mnt 2>&1; echo rc=$?)"
echo "CHECK write-proc-sys $( (echo x > /proc/sys/kernel/hostname) 2>&1; echo rc=$?)"
echo "CHECK read-proc-kcore $( (head -c 16 /proc/kcore | wc -c) 2>&1)"
echo "CHECK read-proc-keys $( (head -c 16 /proc/keys | wc -c) 2>&1)"
echo "CHECK read-proc-sched_debug $( (head -c 16 /proc/sched_debug | wc -c) 2>&1)"
echo "CHECK write-sys $( (touch /sys/c1) 2>&1; echo rc=$?)"
echo "CHECK write-boot $( (mkdir /boot/c1) 2>&1; echo rc=$?)  (outside Landlock's writable paths)"
echo "CHECK write-media $( (touch /media/c1) 2>&1; echo rc=$?)  (outside Landlock's writable paths)"
echo "CHECK write-tmp $( (touch /tmp/c1) 2>&1; echo rc=$?)  (inside)"
echo "CHECK write-usr-local $( (touch /usr/local/c1) 2>&1; echo rc=$?)  (inside)"
echo "CHECK dev-nodes $(ls /dev | tr '\n' ' ')"
echo "CHECK mountinfo-root $(awk '$5=="/"{print $9, $10}' /proc/self/mountinfo | head -1)"
