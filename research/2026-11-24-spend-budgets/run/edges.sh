#!/bin/bash
# usage: edges.sh <label> [case:config:scenario ...]
# Runs each edge case of edges.py under its config, then checks the boundary's record against
# the stand-in's bill (edges_check.py). With no case given, runs them all. BOUNDARY=<binary>
# runs another build of the boundary, such as #374's, to show a case failing before its fix.
set -u
R=$(cd $(dirname $0); pwd); E=$(dirname $R)
LABEL=$1; shift
ALL="prev_id:prev_id:base background:background:base websearch:websearch:base chat:chat-deny:base chat:chat-estimate:base
cache_expiry:cache_expiry:cache gcache:gcache:slow batch:batch:slow midstream:midstream-finish:base midstream:midstream-cut:base
midstream:midstream-typical-finish:base midstream:midstream-typical-cut:base
fork:fork-fresh:base fork:fork-carry:base fork:fork-shared:base"
for spec in ${@:-$ALL}; do
  IFS=: read case conf scen <<< "$spec"
  out=edges-$LABEL-$conf
  CASE=$case $R/run.sh $out $R/configs/edges/$conf.json $R/scenarios/edges/$scen.json 240 edge > /dev/null
  python3 $R/edges_check.py $E/out/$out $case $conf
done
