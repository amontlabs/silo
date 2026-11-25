#!/usr/bin/env bash
# Files the commands read: an access-token file for gcloud, a kubeconfig, manifests, an ARM
# template, an SSH public key nobody holds the private half of. All made up; none is a credential to anything.
set -euo pipefail
mkdir -p /opt/clis/work-aws /opt/clis/work-gcloud /opt/clis/work-az /opt/clis/work-gh /opt/clis/work-kubectl /opt/clis/empty
python3 -c 'import zipfile;z=zipfile.ZipFile("/opt/clis/work-aws/f.zip","w");z.writestr("index.py","def handler(e, c):\n    return 1\n");z.close()'
echo made-up-access-token > /opt/clis/gc-token
cat > /opt/clis/kubeconfig <<K
apiVersion: v1
kind: Config
clusters: [{name: c, cluster: {server: "https://kube.capture.test", certificate-authority: /opt/clis/pki/ca.crt}}]
users: [{name: u, user: {token: made-up}}]
contexts: [{name: c, context: {cluster: c, user: u}}]
current-context: c
K
for r in 0 1; do cat > /opt/clis/work-kubectl/web$([ $r = 0 ] && echo -zero).yaml <<K
apiVersion: apps/v1
kind: Deployment
metadata: {name: web, namespace: prod}
spec:
  replicas: $r
  selector: {matchLabels: {app: web}}
  template:
    metadata: {labels: {app: web}}
    spec: {containers: [{name: web, image: "web:1"}]}
K
done
echo '{"$schema":"https://schema.management.azure.com/schemas/2019-04-01/deploymentTemplate.json#","contentVersion":"1.0.0.0","resources":[]}' > /opt/clis/work-az/t.json
# an SSH public key in wire form with 32 bytes of 9s: a key nobody holds the private half of
python3 -c 'import base64,struct;b=lambda x:struct.pack(">I",len(x))+x;print("ssh-ed25519",base64.b64encode(b(b"ssh-ed25519")+b(b"\x09"*32)).decode(),"laptop")' > /opt/clis/work-gh/key.pub
