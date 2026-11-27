#!/bin/bash
# Makes the test certificates in run/tls: Silo's own authority and the providers' names it signs,
# and a second authority for the stand-in providers, which only the boundary trusts.
set -eu
T=$(cd $(dirname $0); pwd)/tls; mkdir -p $T; cd $T
HOSTS="api.anthropic.com api.openai.com chatgpt.com generativelanguage.googleapis.com api.mistral.ai api.deepseek.com"
SAN=$(for h in $HOSTS; do printf "DNS:%s," $h; done | sed 's/,$//')
for who in silo up; do
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout $who-ca.key -out $who-ca.crt -days 30 \
    -subj "/CN=$who test authority" -addext "basicConstraints=critical,CA:true" -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
  openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout $who-leaf.key -out $who-leaf.csr -subj "/CN=providers" 2>/dev/null
  printf "subjectAltName=%s\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:false\n" "$SAN" > ext.cnf
  openssl x509 -req -in $who-leaf.csr -CA $who-ca.crt -CAkey $who-ca.key -CAcreateserial -out $who-leaf.crt -days 30 -extfile ext.cnf 2>/dev/null
  cat $who-leaf.crt $who-ca.crt > $who-chain.crt
done
cat /etc/ssl/certs/ca-certificates.crt silo-ca.crt > store-with-silo.crt
rm -f *.csr ext.cnf *.srl
ls
