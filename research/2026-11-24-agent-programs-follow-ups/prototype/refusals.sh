#!/bin/bash
cd $(dirname $0)
for v in 400 401 402 403 403:noretry 404 413 429:ra 451 500 200; do ./matrix.sh refuse:$v store 45 cc cx gem oc cl ai go cob; done
echo ALLDONE
