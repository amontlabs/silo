#!/bin/bash
# Two authorities: Silo's (trusted inside each computer) and the fake providers' (trusted only by
# the boundary). One leaf of each covers every host the test uses.
set -e
D=${1:-certs}; mkdir -p $D; cd $D
HOSTS="api.anthropic.com platform.claude.com console.anthropic.com claude.ai auth.openai.com chatgpt.com api.openai.com"
SAN=$(for h in $HOSTS; do printf "DNS:%s," $h; done | sed 's/,$//')
for who in silo up; do
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout $who-ca.key -out $who-ca.crt -days 30 -subj "/CN=$who test authority" -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
  openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout $who-leaf.key -out $who-leaf.csr -subj "/CN=$who leaf" 2>/dev/null
  printf "subjectAltName=%s\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\nkeyUsage=critical,digitalSignature\nauthorityKeyIdentifier=keyid\n" "$SAN" > ext.cnf
  openssl x509 -req -in $who-leaf.csr -CA $who-ca.crt -CAkey $who-ca.key -CAcreateserial -out $who-leaf.crt -days 30 -extfile ext.cnf 2>/dev/null
  cat $who-leaf.crt $who-ca.crt > $who-chain.crt
done
rm -f *.csr *.srl ext.cnf
