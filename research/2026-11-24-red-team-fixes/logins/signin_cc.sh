#!/bin/bash
# Sign Claude Code in, inside computer $1 (a network namespace), with HOME $2. The real program runs
# `claude auth login`; only the person in the browser is played by this script: it reads the
# address the program opens and calls the program's own callback with a code the fake provider
# accepts. The program then exchanges the code at the token endpoint, through the boundary.
N=$1; HM=$2; CA=$3; D=$(mktemp -d); chmod 777 $D
printf '#!/bin/sh\necho "$@" >> %s/u\n' $D > $D/b.sh; chmod 755 $D/b.sh
ip netns exec $N runuser -u cuser -- env -i PATH=/opt/node22/bin:/usr/bin:/bin HOME=$HM NODE_EXTRA_CA_CERTS=$CA \
  DISABLE_TELEMETRY=1 CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 DISABLE_AUTOUPDATER=1 BROWSER=$D/b.sh \
  timeout 40 claude auth login > $D/out 2>&1 < /dev/null & P=$!
for i in $(seq 50); do [ -s $D/u ] && break; sleep 0.2; done
U=$(tail -1 $D/u); PORT=$(echo "$U" | grep -oE 'localhost%3A[0-9]+' | cut -dA -f2); ST=$(echo "$U" | grep -oE 'state=[^&]+' | cut -d= -f2)
ip netns exec $N curl -s -o /dev/null "http://localhost:$PORT/callback?code=code-$RANDOM&state=$ST"
wait $P; tail -1 $D/out; rm -rf $D
