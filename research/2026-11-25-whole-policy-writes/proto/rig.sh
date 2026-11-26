#!/usr/bin/env bash
# Usage: rig.sh setup | teardown
# A test CA, one certificate for every host the CLIs call, and /etc/hosts pointing those hosts at
# 127.0.0.30, where run.py's boundary listens. As #404's rig. Nothing here reaches a real provider.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
W=/opt/wpw
HOSTS="cloudresourcemanager.googleapis.com storage.googleapis.com run.googleapis.com oauth2.googleapis.com www.googleapis.com
iam.googleapis.com serviceusage.googleapis.com
iam.amazonaws.com sts.amazonaws.com sts.us-east-1.amazonaws.com s3.amazonaws.com s3.us-east-1.amazonaws.com
b1.s3.amazonaws.com site.s3.amazonaws.com b1.s3.us-east-1.amazonaws.com site.s3.us-east-1.amazonaws.com kms.us-east-1.amazonaws.com
management.azure.com login.microsoftonline.com identity.capture.test graph.microsoft.com kube.capture.test"
case "$1" in
  setup)
    mkdir -p $W/pki $W/work $W/home; cd $W/pki
    echo $HOSTS | tr ' ' '\n' > hosts.txt
    openssl req -x509 -newkey rsa:2048 -nodes -keyout ca.key -out ca.crt -days 3 -subj "/CN=Silo Whole-Policy Test CA" \
      -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
    openssl req -newkey rsa:2048 -nodes -keyout leaf.key -out leaf.csr -subj "/CN=wpw.test" 2>/dev/null
    printf "subjectAltName=%s\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n" "$(sed 's/^/DNS:/' hosts.txt | paste -sd,)" > ext.cnf
    openssl x509 -req -in leaf.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out leaf.crt -days 3 -extfile ext.cnf 2>/dev/null
    cat leaf.crt ca.crt > chain.crt; cat /etc/ssl/certs/ca-certificates.crt ca.crt > bundle.crt
    cp --update=none /etc/hosts $W/hosts.bak
    { cat $W/hosts.bak; echo "# wpw begin"; echo "127.0.0.30 $(echo $HOSTS | tr '\n' ' ')"; echo "# wpw end"; } > /etc/hosts
    echo made-up-access-token > $W/gc-token
    cat > $W/kubeconfig <<K
apiVersion: v1
kind: Config
clusters: [{name: c, cluster: {server: "https://kube.capture.test", certificate-authority: $W/pki/ca.crt}}]
users: [{name: u, user: {token: made-up}}]
contexts: [{name: c, context: {cluster: c, user: u}}]
current-context: c
K
    echo "$HOSTS" | tr ' \n' ',,' > $W/noproxy
    ;;
  teardown)
    cp $W/hosts.bak /etc/hosts
    ;;
esac
