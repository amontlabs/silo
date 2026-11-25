#!/bin/bash
# usage: matrix.sh <scenario> <ca> <timeout> <prog...> ; runs them in parallel, out/<prog>-<scenario>-<ca>
D=$(cd $(dirname $0); pwd); . $D/progs.sh; S=$1; C=$2; T=$3; shift 3
for p in "$@"; do v=P_$p; spec=${!v}; e=${spec%%|*}; c=${spec#*|}; c=${c//Do_the_task./\"Do the task.\"}
  eval "$D/run.sh $p-${S/:/}-$C $S $C $T -- $e -- $c" > /dev/null 2>&1 &
done; wait
