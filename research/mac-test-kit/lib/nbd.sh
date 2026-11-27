# Disks served over NBD, for test 32. Sourced after lib/common.sh (and lib/ckpt.sh for the
# prototype's server). Two servers:
#   ckpt  the checkpoint engine's prototype, `ckpt nbd-serve` (proto/src/nbd.zig): Silo's own
#         server, on a Unix socket only, no checkpoint
#   py    py/nbdserve.py: the same copy-before-write checkpoint through a control socket, on a
#         Unix socket or TCP; Python, so its cost per write is not Silo's

# nbd_start KIND IMAGE ADDR LOG [CONTROL]: start a server in the background and print its pid once
# it listens. ADDR is a socket path, or tcp:HOST:PORT (py only). Returns 1 when it never listened.
nbd_start() {
  local kind="$1" img="$2" addr="$3" log="$4" ctl="${5:-}" pid i=0 listen
  case "$addr" in
    tcp:*) listen="--tcp ${addr#tcp:}" ;;
    *) rm -f "$addr"; listen="--unix $addr" ;;
  esac
  case "$kind" in
    py)
      # shellcheck disable=SC2086  # $listen is two words on purpose
      if [ -n "$ctl" ]; then
        rm -f "$ctl"
        python3 -I "$KIT_ROOT/py/nbdserve.py" serve --image "$img" $listen --control "$ctl" > "$log" 2>&1 &
      else
        python3 -I "$KIT_ROOT/py/nbdserve.py" serve --image "$img" $listen > "$log" 2>&1 &
      fi
      pid=$! ;;
    ckpt)
      case "$addr" in tcp:*) kit_log "the prototype's server listens on a Unix socket only"; return 1 ;; esac
      "$CKPT_BIN" nbd-serve image="$img" sock="$addr" > "$log" 2>&1 &
      pid=$! ;;
    *) kit_log "nbd_start: unknown server $kind"; return 1 ;;
  esac
  while ! nbd_listening "$addr" "$log"; do
    kill -0 "$pid" 2>/dev/null || { kit_log "the $kind NBD server stopped before listening: $(tail -3 "$log")"; return 1; }
    i=$((i + 1))
    if [ "$i" -gt 50 ]; then kill "$pid" 2>/dev/null; kit_log "the $kind NBD server did not listen in 10 s"; return 1; fi
    sleep 0.2
  done
  echo "$pid"
}

# nbd_listening ADDR LOG: true once the server's socket exists (Unix) or it said so (TCP).
nbd_listening() {
  case "$1" in
    tcp:*) grep -q '"event": "listening"' "$2" 2>/dev/null ;;
    *) [ -S "$1" ] ;;
  esac
}

# nbd_url ADDR: the URL a client is given. The export name is not read by either server.
nbd_url() {
  case "$1" in
    tcp:*) local hp="${1#tcp:}"; echo "nbd://$hp/disk" ;;
    *) echo "nbd+unix:///disk?socket=$1" ;;
  esac
}

# nbd_free_port: a TCP port nobody listens on, on 127.0.0.1.
nbd_free_port() {
  python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

# nbd_stop PID: stop a server; true when it is gone.
nbd_stop() {
  local i=0
  [ -n "${1:-}" ] || return 0
  kill "$1" 2>/dev/null || return 0
  while kill -0 "$1" 2>/dev/null && [ "$i" -lt 25 ]; do sleep 0.2; i=$((i + 1)); done
  if kill -0 "$1" 2>/dev/null; then kill -9 "$1" 2>/dev/null; sleep 0.2; fi
  ! kill -0 "$1" 2>/dev/null
}

# nbd_ctl CONTROL REQUEST...: one request to the py server's control socket; prints its JSON.
nbd_ctl() { python3 -I "$KIT_ROOT/py/nbdserve.py" ctl "$@"; }

# nbd_errors LOG: the server's error lines, one per line (both servers' formats).
nbd_errors() {
  grep -E '"event": "error"|^nbd: [A-Z]|^error|^Traceback|Error: ' "$1" 2>/dev/null | grep -v 'client left' || true
}
