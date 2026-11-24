#!/bin/bash
# Two computers, each a network namespace whose only way out is a veth to the host, where the
# boundary listens. The boundary knows a computer by the address its packets come from, as Silo
# knows it by the channel the packets arrive on.
# usage: net.sh up|down
for i in 1 2; do
  n=c$i
  if [ "$1" = up ]; then
    ip netns add $n
    ip link add v$n type veth peer name e0 netns $n
    ip addr add 10.77.$i.1/24 dev v$n; ip link set v$n up
    ip -n $n addr add 10.77.$i.2/24 dev e0; ip -n $n link set e0 up; ip -n $n link set lo up
    ip -n $n route add default via 10.77.$i.1
    mkdir -p /etc/netns/$n
    printf "127.0.0.1 localhost\n10.77.$i.1 api.anthropic.com platform.claude.com console.anthropic.com claude.ai auth.openai.com chatgpt.com api.openai.com\n" > /etc/netns/$n/hosts
  else
    ip netns del $n 2>/dev/null; ip link del v$n 2>/dev/null; rm -rf /etc/netns/$n
  fi
done
