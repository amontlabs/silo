#!/bin/sh
# Every run of this research, in the order it was made, one call at a time on 4 threads:
#   PY=python3 sh queue.sh
set -e
cd "$(dirname "$0")"
PY=${PY:-python3}
for m in q4b q8b; do
  $PY local.py inline  $m --set clean
  $PY local.py extract $m --set clean
done
$PY local.py inline  q4b --set attacked --only control,a4,a8
$PY local.py extract q4b --set attacked --only control,a4,a8
$PY local.py inline  q8b --set attacked --only a8
$PY local.py extract q8b --set attacked --only a8
