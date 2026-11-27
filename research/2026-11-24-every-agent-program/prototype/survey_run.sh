#!/bin/bash
# usage: survey_run.sh <out dir> [only-name]  ; the listener must run on 443
S=${SILO_EXP:?set SILO_EXP to the folder that holds w/, npm/ and dl/}; W=$S/w; O=$1; mkdir -p $O
P=$S/npm/bin:$S/dl/goose:$S/dl/cursor/dist-package:/root/.local/bin:/opt/node22/bin:/usr/local/bin:/usr/bin:/bin
while IFS='|' read -r name envs cmd; do
  [[ -z "$name" || "$name" == \#* ]] && continue; [ -n "$2" ] && [ "$2" != "$name" ] && continue
  H=$O/home-$name; rm -rf $H; mkdir -p $H/work; echo "== $name" >> $O/marks.txt; date +%s.%N >> $O/marks.txt
  python3 $W/survey.py $O/$name.log $W/tls/chain.crt $W/tls/leaf.key & L=$!; sleep 0.7
  ( cd $H/work; $W/nsrun env -i PATH=$P HOME=$H TERM=dumb $EXTRA $envs timeout ${TO:-45} bash -c "$cmd" < /dev/null > $O/$name.out 2>&1; echo "exit=$?" >> $O/$name.out )
  kill $L; wait $L 2>/dev/null
done < $W/progs.sh
