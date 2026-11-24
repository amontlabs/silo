#!/bin/bash
# usage: run.sh <outname> <scenario> <ca: none|store|env> <timeout> -- <env assignments...> -- <command...>
# Runs one agent program in a private network and mount namespace: its DNS, its 443 and its
# trust store are the fake hub's. Nothing outside the namespace is changed.
set -u
D=$(cd $(dirname $0); pwd); NAME=$1; SCEN=$2; CA=$3; TO=$4; shift 4; [ "$1" = -- ] && shift
ENVS=(); while [ "$1" != -- ]; do ENVS+=("$1"); shift; done; shift
OUTD=$D/out/$NAME; rm -rf $OUTD; mkdir -p $OUTD/home $OUTD/site; echo "old line" > $OUTD/site/existing.txt
(cd $OUTD/site && git init -q && git -c user.email=a@b -c user.name=a commit -q --allow-empty -m init)
printf '%s' "${PROBE_EXTRA:-}" > $OUTD/.probe_extra; printf 'nameserver 127.0.0.1\n' > $OUTD/.resolv; printf '127.0.0.1 localhost\n::1 localhost\n' > $OUTD/.hosts
CAENV=(); [ $CA = env ] && CAENV=(NODE_EXTRA_CA_CERTS=$D/ca/ca.crt SSL_CERT_FILE=$D/ca/store-with-ca.crt REQUESTS_CA_BUNDLE=$D/ca/store-with-ca.crt)
cat > $OUTD/.inner.sh <<IN
set -u
python3 $D/lo_up.py
mount --bind $OUTD/.resolv /etc/resolv.conf; mount --bind $OUTD/.hosts /etc/hosts
[ $CA = store ] && mount --bind $D/ca/store-with-ca.crt /etc/ssl/certs/ca-certificates.crt
mkfifo $OUTD/.ctl; exec 9<>$OUTD/.ctl
env -i PATH=/usr/bin:/bin OUT=$OUTD CA_DIR=$D/ca SCENARIO=$SCEN WORK=$OUTD/site PROBE_FIFO=$OUTD/.ctl PROBE_EXTRA_FILE=$OUTD/.probe_extra ALLOW=$OUTD/home/:/dev/:/tmp/:/proc/ python3 $D/fakehub.py 2>$OUTD/hub.err &
HP=\$!; sleep 1
cd $OUTD/site
START=\$(date +%s.%N)
env -i PATH=$D/bin:/opt/node22/bin:/root/.local/bin:/usr/local/bin:/usr/bin:/bin HOME=$OUTD/home TERM=dumb USER=root ${CAENV[@]:-} ${ENVS[@]:-} timeout $TO ${SPROBE:-$D/sprobe-argv} $OUTD/.ctl $OUTD/probe.log -- "\$@" < /dev/null > $OUTD/out.txt 2> $OUTD/err.txt
echo "exit=\$? secs=\$(echo "\$(date +%s.%N) - \$START" | bc)" > $OUTD/result.txt
kill \$HP
IN
unshare -n -m --propagation private bash $OUTD/.inner.sh "$@"
ls $OUTD/site >> $OUTD/result.txt; cat $OUTD/result.txt
