#!/bin/bash
# The cases in three settings of c2's gvproxy, into results/:
#   handoff-380       #380's hand-off mode as its suite runs it (-outbound-deny private,host)
#   handoff-deny-host the deny list cut to the Mac's own addresses, with -ec2-metadata-access
#   handoff-metadata  the same with gvproxy-ec2-flag.patch applied (BIN2: that build)
R=$(cd $(dirname $0); pwd); E=$(dirname $R); BIN=${BIN:-/root/e2e/bin}; BIN2=${BIN2:-/root/e2e/bin2}
go() { O=/tmp/e2e/cases-$1; BIN=$2 GVPROXY_ARGS="$3 -outbound-handoff $O/c2.handoff.sock -dns-upstream 127.0.0.1:5302" $R/up.sh $O > /dev/null
  python3 $R/cases.py $O > $E/results/cases-$1.txt 2>&1; tail -1 $E/results/cases-$1.txt; $R/down.sh $O; }
go handoff-380 $BIN "-no-gateway-api -no-host-loopback -outbound-deny private,host"
go handoff-deny-host $BIN "-no-gateway-api -no-host-loopback -outbound-deny host -ec2-metadata-access"
go handoff-metadata $BIN2 "-no-gateway-api -no-host-loopback -outbound-deny host -ec2-metadata-access"
