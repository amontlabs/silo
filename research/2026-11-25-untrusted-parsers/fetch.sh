#!/bin/bash
# Puts the drafts' code this folder tests in <dir>, each at the commit it was tested at.
# usage: fetch.sh <dir>     (run inside a clone of amontlabs/silo)
set -e; D=$(mkdir -p $1; cd $1; pwd)
pin() { git fetch -q origin "$2" && mkdir -p $D/$1 && git archive "$2" "${@:3}" | tar -x -C $D/$1; }
pin 385 8f8df9fb074487b5249d1fe6628367ae082b7fd5 research/2026-11-24-spend-budgets       # spend boundary, with #374's code
pin 386 98618262996bb839f7c33fdd5369e02000f0c352 research/2026-11-25-record-format        # record writer and verify
pin 379 906846fbaf8875ef3714295c0d91736063e6209e research/2026-11-25-credential-brokering # SSH agent, metadata server
pin 381 3f0dcd664bcaba9ee01c6325552a99126563b37b research/2026-11-25-linux-main/proto     # ext4 reader
pin 384 4b6cf69c7ec94e4adc0ea856573e7abd83bb1436 research/2026-11-25-port-previews research/2026-11-25-local-models
pin 378 08e74eec393d26ccd3a30b1354644ff923b98124 research/2026-11-25-techniques-from-two-tools/closed-network
pin 380 df198ecdb95593e9ad42a90ca06ff5c69c66c93d research/2026-11-25-gvproxy-component    # gvproxy-silo.patch
ls $D
