#!/usr/bin/env bash
# Every terraform scenario of details/bursts.md, in order. Each destroy starts from a fresh apply.
set -u
cd "$(dirname "$0")"
H=$(pwd)/holds
TF="terraform"
st() { (cd /opt/burst/work && env TF_DATA_DIR=/opt/burst/tfdata /opt/burst/terraform state list) > "outputs/$1.state.txt" 2>&1; }
fresh() { rm -f /opt/burst/work/terraform.tfstate*; ./run.sh "$1" - $TF apply -auto-approve -no-color; }
k() { KEEP_MOTO=1 ./run.sh "$@"; }

fresh apply
k destroy - $TF destroy -auto-approve -no-color
fresh apply-b; k destroy-p1 - $TF destroy -auto-approve -no-color -parallelism=1
fresh apply-c; k replace - $TF apply -auto-approve -no-color -var suffix=-v2
k destroy-after-replace - $TF destroy -auto-approve -no-color -var suffix=-v2
fresh apply-d; k destroy-hold60 "$H/rds-60-403.json" $TF destroy -auto-approve -no-color; st destroy-hold60
k destroy-hold60-again - $TF destroy -auto-approve -no-color; st destroy-hold60-again
fresh apply-e; k destroy-hold90 "$H/rds-90-403.json" $TF destroy -auto-approve -no-color; st destroy-hold90
fresh apply-f; k destroy-holdall30-pass "$H/all-deletes-30-pass.json" $TF destroy -auto-approve -no-color
fresh apply-g; k destroy-hold400 "$H/rds-400-403.json" $TF destroy -auto-approve -no-color; st destroy-hold400
echo ALL DONE
