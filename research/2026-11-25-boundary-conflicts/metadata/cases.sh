#!/bin/bash
# #395's cases with c2's gvproxy as #380 runs it, before and after gvproxy-handoff-decides.patch.
#   before-380     #380's patch, #380's flags (-outbound-deny private,host): the metadata cases fail
#   after-380      the same flags, with gvproxy-handoff-decides.patch on top of #380's patch
#   after-sealed   the same build with -sealed instead (it adds private,host,host-networks)
# usage: cases.sh      (as root, after #395's run/tls.sh and builds; BIN: #380's build, BIN3: with this patch)
H=$(cd $(dirname $0); pwd); R=$H/../../2026-11-25-boundary-end-to-end/run
BIN=${BIN:-/root/e2e/bin}; BIN3=${BIN3:-/root/e2e/bin3}
one() { O=/tmp/e2e/conflicts-$1; BIN=$2 GVPROXY_ARGS="$3 -outbound-handoff $O/c2.handoff.sock -dns-upstream 127.0.0.1:5302" $R/up.sh $O > /dev/null
  python3 $R/cases.py $O > $H/results/cases-$1.txt 2>&1; echo "$1: $(tail -1 $H/results/cases-$1.txt)"; $R/down.sh $O; }
one before-380 $BIN "-no-gateway-api -no-host-loopback -outbound-deny private,host"
one after-380 $BIN3 "-no-gateway-api -no-host-loopback -outbound-deny private,host"
one after-sealed $BIN3 "-sealed"
