#!/bin/bash
# usage: measure.sh <work dir>
# Fetches LiteLLM's history (blobs of one file only) and OpenRouter's catalogue, then prints the
# measurements of details/history.md into results/.
set -eu
H=$(cd $(dirname $0); pwd); W=$1; mkdir -p $W; F=model_prices_and_context_window.json
[ -d $W/litellm ] || git clone -q --filter=blob:none --no-checkout https://github.com/BerriAI/litellm $W/litellm
git -C $W/litellm log --first-parent --format='%H %cs' -- $F | while read h d; do echo "$(git -C $W/litellm rev-parse $h:$F) $h $d"; done > $W/blobs.txt
# One fetch per 300 blobs instead of one per blob.
cut -d' ' -f1 $W/blobs.txt | sort -u | split -l 300 - $W/bpart_
for p in $W/bpart_*; do git -C $W/litellm -c fetch.negotiationAlgorithm=noop fetch -q --no-tags --filter=blob:none origin $(cat $p); done
python3 $H/walk_litellm.py $W/litellm $W/blobs.txt $W/events.json
curl -s https://openrouter.ai/api/v1/models > $W/openrouter.json
mkdir -p $H/../results
python3 $H/history.py $W/events.json $H/labels.json > $H/../results/history.txt
python3 $H/catalogue.py $W/openrouter.json $W/events.json > $H/../results/catalogue.txt
