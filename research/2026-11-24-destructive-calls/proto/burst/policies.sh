#!/usr/bin/env bash
# Live runs of each grouping policy, with a simulated user who answers yes 8 s after looking.
set -u
cd "$(dirname "$0")"
P=$(pwd)/policies
TF=terraform
while pgrep -f scenarios.sh >/dev/null; do sleep 5; done
fresh() { rm -f /opt/burst/work/terraform.tfstate*; POLICY= ./run.sh "$1" - $TF apply -auto-approve -no-color; }
for pol in computer host program connection request; do
  fresh "apply-pol-$pol"
  POLICY=$P/$pol.json KEEP_MOTO=1 ./run.sh "pol-$pol-destroy" - $TF destroy -auto-approve -no-color
done
POLICY= ./s3.sh s3rm-500 - rm 500
for pol in computer connection; do POLICY=$P/$pol.json ./s3.sh "pol-$pol-s3rm" - rm 500; done
POLICY=$P/computer.json ./s3.sh pol-computer-seq20 - seq 20
POLICY=$P/computer-slow.json ./s3.sh pol-slow-s3rm30 - rm 30
POLICY=$P/computer-slow-noretry.json ./s3.sh pol-slow-noretry-s3rm30 - rm 30
./s3.sh s3rm-hold30-403 "$(pwd)/holds/s3obj-30-403-first.json" rm 50
./s3.sh s3rm-hold70-pass "$(pwd)/holds/s3obj-70-pass-first.json" rm 50
echo POLICIES DONE
