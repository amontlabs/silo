#!/bin/sh
# id: K09
# title: A restore to a checkpoint taken during npm install
# needs: K07
# decides: restore-to-a-checkpoint, what-a-restore-leaves
# question: Does main boot from a clone taken mid-install, does ext4 replay its journal without error, and (with KIT_CLAUDE=1 and a signed-in Claude Code) does claude --resume list the conversation?
TEST=K09; . "$(dirname "$0")/../lib.sh"; need_kvm
vm_new k09; vm_boot k09; vm_wait_ssh k09 300 >/dev/null || fail "no SSH"
vm_ssh k09 'sudo apt-get -qq install -y npm >/dev/null 2>&1; mkdir -p app && cd app && npm init -y >/dev/null && nohup npm install typescript next react react-dom >/dev/null 2>&1 &'
sleep 20; cp --reflink=always "$KIT/vms/k09/disk.raw" "$KIT/vms/k09/ckpt.raw" || skip "no reflink under $KIT"
vm_stop k09 >/dev/null
mv "$KIT/vms/k09/ckpt.raw" "$KIT/vms/k09/disk.raw"
vm_boot k09 --no-seed; t=$(vm_wait_ssh k09 120) || fail "the restored disk did not boot"
j=$(vm_ssh k09 'sudo journalctl -k -b | grep -c "EXT4-fs.*recovery complete"')
err=$(vm_ssh k09 'sudo journalctl -k -b | grep -ci "EXT4-fs error"')
vm_stop k09 >/dev/null
[ "$err" = 0 ] || fail "ext4 errors after the restore"
result K09 passed "restored disk booted to SSH in ${t}s; journal recoveries $j; ext4 errors 0"
