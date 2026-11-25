# start_standin out-dir: (re)start the stand-in with $PORTS, writing to that folder.
# The stand-in's process id is kept in /tmp/forkruns/standin.pid.
stop_standin() { [ -f /tmp/forkruns/standin.pid ] && kill "$(cat /tmp/forkruns/standin.pid)" 2>/dev/null; rm -f /tmp/forkruns/standin.pid; sleep 0.3; }
start_standin() {
  stop_standin; mkdir -p "$1"
  OUT="$1" PORTS="$PORTS" setsid python3 "$R/standin/provider.py" > "$1/standin.log" 2>&1 < /dev/null &
  echo $! > /tmp/forkruns/standin.pid
  for i in $(seq 50); do grep -q listening "$1/standin.log" 2>/dev/null && return 0; sleep 0.1; done; return 1
}
