#!/bin/bash
# Sign Codex in, inside computer $1, with HOME $2: the real `codex login`; only the person in the
# browser is played by this script, which calls the program's callback with a code.
N=$1; HM=$2; CA=$3; D=$(mktemp -d); chmod 777 $D; mkdir -p $HM/.codex; chown -R cuser $HM
printf '#!/bin/sh\necho "$@" >> %s/u\n' $D > $D/b.sh; chmod 755 $D/b.sh
ip netns exec $N runuser -u cuser -- env -i PATH=/opt/codex:/usr/bin:/bin HOME=$HM CODEX_HOME=$HM/.codex SSL_CERT_FILE=$CA CODEX_CA_CERTIFICATE=$CA BROWSER=$D/b.sh \
  timeout 40 codex login > $D/out 2>&1 < /dev/null & P=$!
for i in $(seq 50); do [ -s $D/u ] && break; sleep 0.2; done
U=$(tail -1 $D/u); ST=$(echo "$U" | grep -oE 'state=[^&]+' | cut -d= -f2)
ip netns exec $N curl -s -o /dev/null "http://127.0.0.1:1455/auth/callback?code=code-$RANDOM&state=$ST"
for i in $(seq 50); do [ -s $HM/.codex/auth.json ] && break; sleep 0.2; done; sleep 0.5; kill $P 2>/dev/null
[ -s $HM/.codex/auth.json ] && echo "signed in" || { echo "sign-in failed"; cat $D/out; }; rm -rf $D
