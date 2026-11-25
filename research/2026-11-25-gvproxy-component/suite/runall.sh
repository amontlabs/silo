#!/bin/bash
# Every mode against every case, then the verdict per case against expected.txt.
# usage: runall.sh <bin-dir> [mode...]    modes: stock sealed363 sealed closed boundary boundary-down
# boundary-down: gvproxy hands off to a boundary that is not running, as when Silo's boundary stops.
H=$(cd $(dirname $0); pwd); B=$1; shift; MODES=${@:-stock sealed363 sealed closed boundary boundary-down}
OUT=${WORK:-/tmp/gvsuite}/results; mkdir -p $OUT
for m in $MODES; do
  case $m in
    stock)     $H/run.sh $m $B/gvproxy-stock ;;
    sealed363) $H/run.sh $m $B/gvproxy-363 -sealed ;;
    sealed)    $H/run.sh $m $B/gvproxy-silo -sealed ;;
    closed)    $H/run.sh $m $B/gvproxy-silo -closed ;;
    boundary)
      L=${WORK:-/tmp/gvsuite}/boundary.log; rm -f $L
      $B/standin-boundary -sock /tmp/silo-boundary.sock -log $L -dns 127.0.0.1:5353 -answer allowed.example=203.0.113.10 & BP=$!
      sleep 0.3
      BOUNDARY_LOG=$L $H/run.sh $m $B/gvproxy-silo -no-gateway-api -no-host-loopback -outbound-deny private,host \
        -outbound-handoff /tmp/silo-boundary.sock -dns-upstream 127.0.0.1:5353
      kill $BP ;;
    boundary-down)
      rm -f /tmp/silo-boundary.sock
      $H/run.sh $m $B/gvproxy-silo -no-gateway-api -no-host-loopback -outbound-deny private,host \
        -outbound-handoff /tmp/silo-boundary.sock -dns-upstream 127.0.0.1:5353 ;;
  esac > $OUT/$m.txt 2>&1
done
python3 -I $H/verdict.py $H/expected.txt $OUT $MODES
