#!/bin/zsh
# usage: verify.sh <sandbox> ws|home  — compares a guest tree manifest with the old computer's
. ~/.s2/env.sh; M=$MSB_PATH; S=${0:A:h}
which=$2; dir=/workspace; [ $which = home ] && dir=/home/silo
$M cp $S/manifest.py $1:/tmp/manifest.py >/dev/null
$M exec $1 --user root -- sh -c "python3 /tmp/manifest.py $dir > /tmp/new.manifest"
$M cp $1:/tmp/new.manifest $S/new-$1-$which.manifest >/dev/null
f=old-$which.manifest; [ $which = home ] && f=old-home.manifest
# ignore files we expect to change by design (record, atime)
diff <(sort $S/$f) <(sort $S/new-$1-$which.manifest) | sed 's/|blocks.*//' | cut -c1-220 | head -${3:-12}
echo "lines old=$(wc -l < $S/$f) new=$(wc -l < $S/new-$1-$which.manifest) difflines=$(diff <(sort $S/$f) <(sort $S/new-$1-$which.manifest) | grep -c '^[<>]')"
