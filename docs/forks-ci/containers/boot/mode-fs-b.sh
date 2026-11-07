#!/bin/bash
# Container B, a fork of A: sees A's changes, then makes its own.
echo "FS_B_SEES files=$(find /workspace -type f | wc -l) big=$(stat -c %s /workspace/big.bin) a=$(cat /root/a.txt) marker=$(test -e /etc/c1-marker && echo yes || echo no) notices=$(test -e /usr/share/doc/silo-guest-third-party/NOTICES.md && echo present || echo gone)"
echo "edited by B" >> /workspace/proj/f1.txt
rm /workspace/proj/f2.txt
echo "new in B" > /workspace/proj/b-new.txt
rm /etc/c1-marker
echo "FS_B done"
