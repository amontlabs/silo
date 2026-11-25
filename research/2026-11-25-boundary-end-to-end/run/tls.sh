#!/bin/bash
# Test certificates, in run/tls: Silo's authority (the boundary signs a certificate per host with
# it, using silo-leaf.key) and the stand-in hosts' authority, which only the boundary trusts.
set -eu
T=$(cd $(dirname $0); pwd)/tls; mkdir -p $T; cd $T
HOSTS="api.anthropic.com api.openai.com chatgpt.com generativelanguage.googleapis.com storage.googleapis.com oauth2.googleapis.com management.azure.com login.microsoftonline.com github.com"
SAN=$(for h in $HOSTS; do printf "DNS:%s," $h; done | sed 's/,$//')
for who in silo up; do
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout $who-ca.key -out $who-ca.crt -days 30 \
    -subj "/CN=$who test authority" -addext "basicConstraints=critical,CA:true" -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
done
openssl genpkey -algorithm ec -pkeyopt ec_paramgen_curve:P-256 -out silo-leaf.key 2>/dev/null
openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout up-leaf.key -out up-leaf.csr -subj "/CN=hosts" 2>/dev/null
printf "subjectAltName=%s\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:false\n" "$SAN" > ext.cnf
openssl x509 -req -in up-leaf.csr -CA up-ca.crt -CAkey up-ca.key -CAcreateserial -out up-leaf.crt -days 30 -extfile ext.cnf 2>/dev/null
cat up-leaf.crt up-ca.crt > up-chain.crt
cat /etc/ssl/certs/ca-certificates.crt silo-ca.crt > store-with-silo.crt
rm -f *.csr ext.cnf *.srl
