#!/bin/bash
# Container A of the fork test: changes that land in its upper layer.
mkdir -p /workspace/proj
for i in $(seq 1 200); do echo "file $i" > "/workspace/proj/f$i.txt"; done
dd if=/dev/urandom of=/workspace/big.bin bs=1M count=100 status=none
echo "from A" > /root/a.txt
echo "# edited by A" >> /etc/motd
touch /etc/c1-marker
rm /usr/share/doc/silo-guest-third-party/NOTICES.md
echo "FS_A files=$(find /workspace -type f | wc -l)"
