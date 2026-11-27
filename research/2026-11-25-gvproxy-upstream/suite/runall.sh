#!/bin/bash
# Every mode against every case, then the verdict per case against expected.txt, which holds what
# a correct gvproxy does: a stock result that differs from it is a bug of stock gvproxy.
# usage: runall.sh <bin-dir> [mode...]    (as root)
#   stock               stock gvproxy, no options
#   stock-ec2           stock gvproxy, -ec2-metadata-access
#   ec2                 the series, -ec2-metadata-access
#   on-main             the five patches on upstream main, -no-gateway-api -no-host-loopback
#   sealed              the series, -sealed (Silo's fork-only shorthand)
#   explicit            the series, the options -sealed stands for, written out
#   closed              the series, -closed
#   boundary            the series, hand-off to the stand-in boundary, private networks denied
#   boundary-metadata   #395's setting: hand-off, only the host's addresses and networks denied,
#                       -ec2-metadata-access, so the boundary can serve #379's metadata
#   boundary-down       the series, hand-off to a boundary that is not running
H=$(cd $(dirname $0); pwd); B=$1; shift
MODES=${@:-stock stock-ec2 ec2 on-main sealed explicit closed boundary boundary-metadata boundary-down}
OUT=${WORK:-/tmp/gvsuite}/results; mkdir -p $OUT
SOCK=/tmp/silo-boundary.sock; L=${WORK:-/tmp/gvsuite}/boundary.log
DENY="-outbound-deny private,host,host-networks"
HANDOFF="-outbound-handoff $SOCK -dns-upstream 127.0.0.1:5353"
boundary() { rm -f $L $SOCK; $B/standin-boundary -sock $SOCK -log $L -dns 127.0.0.1:5353 -answer allowed.example=203.0.113.10 "$@" & BP=$!; sleep 0.3; }
for m in $MODES; do
  case $m in
    stock)      $H/run.sh $m $B/gvproxy-stock ;;
    stock-ec2)  $H/run.sh $m $B/gvproxy-stock -ec2-metadata-access ;;
    ec2)        $H/run.sh $m $B/gvproxy-series -ec2-metadata-access ;;
    on-main)    $H/run.sh $m $B/gvproxy-on-main -no-gateway-api -no-host-loopback ;;
    sealed)     $H/run.sh $m $B/gvproxy-series -sealed ;;
    explicit)   $H/run.sh $m $B/gvproxy-series -no-gateway-api -no-host-loopback $DENY ;;
    closed)     $H/run.sh $m $B/gvproxy-series -closed ;;
    boundary)   boundary; BOUNDARY_LOG=$L $H/run.sh $m $B/gvproxy-series -no-gateway-api -no-host-loopback $DENY $HANDOFF; kill $BP ;;
    boundary-metadata)
                # The stand-in refuses private destinations by default; here gvproxy alone must hold them.
                boundary -deny-private=false; BOUNDARY_LOG=$L $H/run.sh $m $B/gvproxy-series -no-gateway-api -no-host-loopback \
                  -outbound-deny host,host-networks -ec2-metadata-access $HANDOFF; kill $BP ;;
    boundary-down) rm -f $SOCK; $H/run.sh $m $B/gvproxy-series -no-gateway-api -no-host-loopback $DENY $HANDOFF ;;
  esac > $OUT/$m.txt 2>&1
done
python3 -I $H/verdict.py $H/expected.txt $OUT $MODES
