#!/bin/bash
# Runs a command inside computer c1 or c2 of a run, with a clean environment: c1 gets the proxy
# settings #378 gives main, c2 gets none (everything it sends goes through gvproxy's hand-off).
# Both trust Silo's authority. usage: in.sh <out> c1|c2 <command...>
OUT=$1; C=$2; shift 2; . $OUT/env
E=(PATH=/opt/node22/bin:/usr/local/bin:/usr/bin:/bin HOME=$OUT/home-$C TERM=dumb SSL_CERT_FILE=$TLS/store-with-silo.crt NODE_EXTRA_CA_CERTS=$TLS/silo-ca.crt CURL_CA_BUNDLE=$TLS/store-with-silo.crt REQUESTS_CA_BUNDLE=$TLS/store-with-silo.crt)
mkdir -p $OUT/home-$C
if [ $C = c1 ]; then
  E+=(HTTPS_PROXY=http://127.0.0.1:3128 HTTP_PROXY=http://127.0.0.1:3128 https_proxy=http://127.0.0.1:3128 http_proxy=http://127.0.0.1:3128 NODE_USE_ENV_PROXY=1 NO_PROXY=localhost,127.0.0.1)
  exec ip netns exec $NS1 env -i "${E[@]}" "$@"
fi
exec ip netns exec $NS2 env -i "${E[@]}" "$@"
