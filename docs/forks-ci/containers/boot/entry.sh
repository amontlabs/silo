#!/bin/bash
# The container's first shell (started by silo-init, so already under Landlock). Prints UP when it runs, waits for
# the host to finish the network (the default route in /proc/net/route) and prints READY: a ready shell.
# Then runs /silo/mode-$1.sh. With C1_HOLD=1 it stays alive afterwards so the host can measure and inspect.
mode=$1
shift
echo "UP $(date +%s.%N)"
if [ -n "${C1_NET:-}" ]; then
  for _ in $(seq 1 3000); do
    awk '$1=="eth0" && $2=="00000000"{f=1} END{exit !f}' /proc/net/route 2>/dev/null && break
    sleep 0.01
  done
fi
echo "READY $(date +%s.%N)"
bash "/silo/mode-$mode.sh" "$@"
rc=$?
echo "MODE_EXIT $rc $(date +%s.%N)"
if [ "${C1_HOLD:-0}" = 1 ]; then
  trap 'exit 0' TERM INT
  while :; do sleep 1; done
fi
exit "$rc"
