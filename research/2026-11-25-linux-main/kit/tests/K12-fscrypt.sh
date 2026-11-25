#!/bin/sh
# id: K12
# title: fscrypt on a folder in main gives a gap, never "nothing changed"
# needs: K07
# decides: file-changes-from-outside
TEST=K12; . "$(dirname "$0")/../lib.sh"; need_kvm
vm_new k12; vm_boot k12; vm_wait_ssh k12 300 >/dev/null || fail "no SSH"
vm_ssh k12 'sudo apt-get -qq install -y fscrypt >/dev/null 2>&1; sudo tune2fs -O encrypt $(findmnt -no SOURCE /) >/dev/null; sudo fscrypt setup --quiet --force >/dev/null 2>&1; mkdir -p ~/secret && echo pass | fscrypt encrypt ~/secret --source=custom_passphrase --name=k --quiet >/dev/null 2>&1; echo x > ~/secret/a; sync'
sleep 10; cp --reflink=always "$KIT/vms/k12/disk.raw" "$KIT/vms/k12/c.raw" || skip "no reflink"
vm_stop k12 >/dev/null
off=$(python3 -I -c "import struct,sys;f=open(sys.argv[1],'rb');f.seek(512+72);l,n,s=struct.unpack('<QII',f.read(16));f.seek(l*512+32);print(struct.unpack('<Q',f.read(8))[0]*512)" "$KIT/vms/k12/c.raw")
r=$(KIT_FS_OFFSET=$off python3 -I "$KIT_SRC/../proto/ext4read.py" walk "$KIT/vms/k12/c.raw" 2>&1 | tail -1)
case $r in gap*) result K12 passed "the trial reader refused: $r (Silo's reader gives a gap for the folder only)" ;; *) result K12 failed "the reader read on: $r" ;; esac
