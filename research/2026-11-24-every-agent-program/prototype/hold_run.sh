#!/bin/bash
# Each program in its own network and mount namespace, with a listener on 443 that never answers
# a model request. usage: hold_run.sh <out> <seconds> <name>...
S=${SILO_EXP:?set SILO_EXP to the folder that holds w/, npm/ and dl/}; W=$S/w; O=$1; T=$2; shift 2; mkdir -p $O
P=$S/npm/bin:$S/dl/goose:$S/dl/cursor/dist-package:/root/.local/bin:/opt/node22/bin:/usr/local/bin:/usr/bin:/bin
for name in "$@"; do
  line=$(grep "^$name|" $W/progs.sh); envs=$(echo "$line" | cut -d'|' -f2); cmd=$(echo "$line" | cut -d'|' -f3- | sed 's/Say hi/Write a long essay on rivers./')
  H=$O/home-$name; rm -rf $H; mkdir -p $H/work
  unshare -n -m --propagation private bash -c "
    mount --bind $W/hosts.test /etc/hosts; python3 $W/lo_up.py
    HOLD=1 python3 $W/survey.py $O/$name.log $W/tls/chain.crt $W/tls/leaf.key & sleep 0.7
    cd $H/work; s=\$(date +%s.%N); env -i PATH=$P HOME=$H TERM=dumb $envs timeout $T bash -c '$cmd' < /dev/null > $O/$name.out 2>&1; e=\$?; echo \"exit=\$e after \$(python3 -c \"print(round(\$(date +%s.%N)-\$s,1))\")s\" >> $O/$name.out
    kill %1" &
done
wait
