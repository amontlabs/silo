#!/bin/bash
# id: 20
# title: The engine's socket: who connected, and what exec changes
# tier: quick
# time: about 2 minutes
# needs: ./kit build (c/peercred.c), no VM, no download; sudo only with --allow-sudo, for the other-user half (otherwise skipped)
# step: 1
# decides: who-can-call-the-engine, reaching-a-computer
# question: What does a Unix socket server on the Mac learn about the process that connected (user, pid, audit token, the parent chain up to an app), does the audit token's pid version change when that process runs exec, and is a process of another user kept out?
# outcome identity-holds: the caller's user and pid are right in every look, the audit token's pid version changes on exec (so a token kept from accept tells an exec'd caller apart), and another user is kept out.
#   who-can-call-the-engine: same user by the socket's folder and getpeereid is sound; "opened by" can come from LOCAL_PEERPID's parent chain, which the sentence says reaches the app or not (a double fork loses it).
# outcome pidversion-stays: the audit token's pid version stays the same across exec: a check of the caller's program over a Unix socket can be fooled by exec. who-can-call-the-engine: never check a code
#   signature over the socket; use XPC with a peer requirement if that check is ever wanted.
# outcome other-user-not-refused: a process of another user connected and got an answer. who-can-call-the-engine's "refused before any handler runs" does not hold as built here: look at the facts before
#   relying on the socket's folder or getpeereid.
# outcome inconclusive: a look failed (the facts say which), or there is no audit token on this system (the Linux analogue).
. "$KIT_ROOT/lib/common.sh"

W="$KIT_WORK"
PC="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin/peercred"
LINUX=0
if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then LINUX=1; else kit_need_mac; fi
[ -x "$PC" ] || kit_skip "peercred is not built: run ./kit build"

OTHER_DIR=""
SPID=""; OPID=""
cleanup() {
  [ -n "$SPID" ] && kill "$SPID" 2>/dev/null
  [ -n "$OPID" ] && kill "$OPID" 2>/dev/null
  [ -n "$OTHER_DIR" ] && rm -rf "$OTHER_DIR"
  rm -rf "$W/sock"
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

mkdir -p "$W/sock"; chmod 700 "$W" "$W/sock"
S="$W/sock/engine.sock"
: > "$W/serve.jsonl"; : > "$W/clients.jsonl"; : > "$W/other.jsonl"

wait_sock() { local i=0; while [ ! -S "$1" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done; [ -S "$1" ]; }
wait_pid() { local i=0; while kill -0 "$1" 2>/dev/null && [ "$i" -lt "$2" ]; do sleep 0.2; i=$((i + 1)); done; ! kill -0 "$1" 2>/dev/null; }

# The engine stand-in: a socket of mode 0600 in a folder of mode 0700, refusing other users.
"$PC" serve "$S" --max 4 --refuse-other-users --wait-ms 1200 > "$W/serve.jsonl" 2> "$W/serve.log" &
SPID=$!
wait_sock "$S" || kit_fail "peercred serve did not listen: $(cat "$W/serve.log")"
kit_fact socket_mode "$(ls -l "$S" | awk '{print $1}') in a folder $(ls -ld "$W/sock" | awk '{print $1}')"

# 1. A caller started by this test (its chain goes up through the kit to your terminal).
"$PC" client "$S" --label direct >> "$W/clients.jsonl"
# 2. A caller that runs exec on itself after its first message.
"$PC" client "$S" --label exec-self --exec-self >> "$W/clients.jsonl"
# 3. A caller that runs exec on another program (/bin/sleep), which keeps the connection.
"$PC" client "$S" --label exec-sleep --exec-sleep >> "$W/clients.jsonl"
# 4. A caller whose parent exits before it connects (a double fork, as a daemon does).
( ( sleep 1; exec "$PC" client "$S" --label orphan >> "$W/clients.jsonl" ) & )
wait_pid "$SPID" 75 || kit_note "the server did not see four connections in 15 s"
SPID=""

# The other-user half: a second server on an open socket in a folder another user can reach.
if [ "$LINUX" = 1 ] && [ "$(id -u)" = 0 ] && command -v setpriv >/dev/null 2>&1; then
  AS_OTHER="setpriv --reuid=65534 --regid=65534 --clear-groups"
elif [ "$LINUX" = 0 ] && [ "${KIT_SUDO:-0}" = 1 ]; then
  AS_OTHER="sudo"
else
  AS_OTHER=""
  kit_note "the other-user half was skipped: it needs --allow-sudo (it runs peercred as the user nobody)"
fi
if [ -n "$AS_OTHER" ]; then
  if [ "$LINUX" = 0 ]; then
    kit_confirm "For the other-user half, the test makes a folder /private/tmp/silokit-20.XXXXXX (mode 0755) with a copy of peercred and an open socket (mode 0777), runs peercred as the user nobody with sudo against it and against the engine's real socket, and deletes the folder at the end."
  fi
  OTHER_DIR="$(mktemp -d /tmp/silokit-20.XXXXXX)" || kit_fail "mktemp failed"
  chmod 755 "$OTHER_DIR"
  cp "$PC" "$OTHER_DIR/peercred"; chmod 755 "$OTHER_DIR/peercred"
  "$PC" serve "$OTHER_DIR/open.sock" --max 1 --refuse-other-users --mode 0777 >> "$W/other.jsonl" 2> "$W/other-serve.log" &
  OPID=$!
  wait_sock "$OTHER_DIR/open.sock" || kit_fail "the open server did not listen"
  # The real engine socket again, to show the folder keeps the other user out before any check.
  "$PC" serve "$S" --max 1 --refuse-other-users > "$W/serve-again.jsonl" 2>/dev/null &
  SPID=$!
  wait_sock "$S"
  if [ "$LINUX" = 1 ]; then
    $AS_OTHER "$OTHER_DIR/peercred" client "$S" --label other-private >> "$W/other.jsonl" 2>/dev/null
    $AS_OTHER "$OTHER_DIR/peercred" client "$OTHER_DIR/open.sock" --label other-open >> "$W/other.jsonl" 2>/dev/null
  else
    kit_sudo "run peercred as the user nobody, to see the engine's socket keep another user out" \
      -u nobody "$OTHER_DIR/peercred" client "$S" --label other-private >> "$W/other.jsonl"
    kit_sudo "the same against an open socket, to see the server's own check refuse it" \
      -u nobody "$OTHER_DIR/peercred" client "$OTHER_DIR/open.sock" --label other-open >> "$W/other.jsonl"
  fi
  wait_pid "$OPID" 25; OPID=""
  kill "$SPID" 2>/dev/null; SPID=""
fi

for f in serve.jsonl clients.jsonl other.jsonl; do kit_attach "$W/$f"; done
res="$(python3 -I "$KIT_ROOT/py/20-analyze.py" "$W/serve.jsonl" "$W/clients.jsonl" "$W/other.jsonl" "$(id -u)")" ||
  kit_fail "20-analyze.py failed"
key=""; text=""
while IFS="$(printf '\t')" read -r kind a b c; do
  case "$kind" in
    fact) kit_fact "$a" "$b" ;;
    measure) kit_measure "$a" "$b" "$c" ;;
    outcome) key="$a"; text="$b" ;;
  esac
done <<EOF
$res
EOF
[ "$LINUX" = 1 ] && text="Linux analogue (SO_PEERCRED, SO_PEERPIDFD), not a Mac result: $text"
kit_outcome "${key:-inconclusive}" "${text:-no analysis}"
