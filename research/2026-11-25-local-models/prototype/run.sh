#!/bin/bash
# The gate's tests, against stand-ins of Ollama and LM Studio. Needs Python 3 only.
# usage: ./run.sh <out>      Output: <out>/results.txt, <out>/record.jsonl (the gate's record), <out>/mac.jsonl
H=$(cd $(dirname $0); pwd); python3 $H/tests.py ${1:-$H/out}
