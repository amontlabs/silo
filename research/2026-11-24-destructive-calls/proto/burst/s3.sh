#!/usr/bin/env bash
# The aws s3 scenarios: 500 objects removed with `aws s3 rm --recursive`, and a script that
# deletes objects one at a time. `./s3.sh <name> <holds.json|-> rm|seq [count]`; POLICY as in run.sh.
set -u
cd "$(dirname "$0")"
name=$1; holds=$2; mode=$3; n=${4:-500}
mkdir -p /opt/burst/objs && rm -f /opt/burst/objs/*
for i in $(seq -w 1 "$n"); do echo "backup part $i" > /opt/burst/objs/part-$i.dump; done
A="aws --endpoint-url http://127.0.0.1:4566"
# fill the bucket through the proxy, without holds, into a separate trace
POLICY= ./run.sh "$name-fill" - bash -c "$A s3 mb s3://prod-backups && $A s3 cp --recursive --quiet /opt/burst/objs s3://prod-backups/db/" >/dev/null
if [ "$mode" = rm ]; then
  KEEP_MOTO=1 ./run.sh "$name" "$holds" $A s3 rm --recursive s3://prod-backups/db/
else
  KEEP_MOTO=1 ./run.sh "$name" "$holds" bash -c "for k in \$($A s3api list-objects-v2 --bucket prod-backups --query 'Contents[].Key' --output text); do $A s3api delete-object --bucket prod-backups --key \$k || echo FAILED \$k; done"
fi
