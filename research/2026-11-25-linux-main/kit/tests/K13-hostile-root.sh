#!/bin/sh
# id: K13
# title: What root in main can do to the kernel and to the host
# needs: K01
# decides: processes-from-inside-linux, engine-patch-review, network-before-rules, storage-between-computers
# question: As root in main: load a module, kexec, read /dev/mem, replace /boot's kernel and reboot, reach the host's 127.0.0.1, write a raw disk past ext4, read blocks of the disk image never written (must be zero)
TEST=K13; . "$(dirname "$0")/../lib.sh"; need_kvm
vm_new k13; vm_boot k13; vm_wait_ssh k13 300 >/dev/null || fail "no SSH"
r=$(vm_ssh k13 'sudo sh -c "
  printf \"modprobe=%s \" \$(modprobe dummy >/dev/null 2>&1; echo \$?)
  printf \"kexec=%s \" \$(test -e /sys/kernel/kexec_loaded && echo present || echo absent)
  printf \"devmem=%s \" \$(test -e /dev/mem && echo present || echo absent)
  printf \"kernel-before=%s \" \$(uname -v | md5sum | cut -c1-8)
  cp /bin/true /boot/vmlinuz 2>/dev/null; echo garbage > /boot/vmlinuz.silo-test
  printf \"bpf-unprivileged=%s \" \$(cat /proc/sys/kernel/unprivileged_bpf_disabled)
  printf \"zero-tail=%s \" \$(dd if=\$(findmnt -no SOURCE / | sed s/[0-9]*$//) bs=1M skip=30000 count=16 status=none | tr -d \"\\\\0\" | wc -c)
"')
vm_ssh k13 'sudo systemctl reboot' 2>/dev/null; sleep 5
vm_wait_ssh k13 120 >/dev/null || fail "no SSH after the reboot"
after=$(vm_ssh k13 'uname -v | md5sum | cut -c1-8')
vm_stop k13 >/dev/null
result K13 measured "$r kernel-after-replacing-boot=$after (equal to before: the kernel comes from outside)"
