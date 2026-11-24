#!/bin/bash
# id: 08
# title: Claude desktop and the Codex app through an included SSH entry with fd passing
# tier: guided
# time: about 20 minutes
# needs: you at the keyboard, Claude desktop and/or the Codex app (or the ChatGPT app with Codex) installed and signed in, the base computer of test 00, ./kit build (c/sshproxy.c); one line added to ~/.ssh/config for the length of the test
# decides: reaching-a-computer, computer-reached-over-ssh, agent-programs-reach-every-computer
# question: Does the Codex app list a host that only an Included file defines, and do Claude desktop and the Codex app connect through a ProxyCommand that hands ssh its socket (ProxyUseFdpass), as Silo's entry would?
# outcome include-and-fdpass-work: the Codex app lists the included host, and each app tried connects through the fd-passing ProxyCommand. reaching-a-computer: take "one line that includes Silo's file"; the SSH
#   transport: a Unix socket per computer and a ProxyCommand with ProxyUseFdpass, as research 06 proposes.
# outcome codex-ignores-include: the Codex app does not list a host defined only in the included file. reaching-a-computer: Silo must write each computer's Host lines into ~/.ssh/config itself (or the
#   person pastes them), or the Codex app never lists a computer; the fd-passing results are in the sentence.
# outcome fdpass-refused: an app connects through the relaying ProxyCommand but not through the fd-passing one. The SSH transport: Silo's ProxyCommand relays bytes for the length of each session instead of
#   handing ssh the socket (it stays running; Silo still sees each connection).
# outcome proxycommand-refused: an app connects through neither ProxyCommand. The SSH transport: Silo needs a localhost port shown as a channel, for that app at least.
# outcome inconclusive: no app was tried, or the answers were skipped; or the automatic checks failed (the facts say which).
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"

W="$KIT_WORK"
BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin"
PROXY="$BIN/sshproxy"
LINUX=0
if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then LINUX=1; fi
[ -x "$PROXY" ] || kit_skip "sshproxy is not built: run ./kit build"
kit_need_cmd ssh "it ships with macOS"
mkdir -p "$W"; chmod 700 "$W"

HOSTN=silo-kit-main
CONFIG="$HOME/.ssh/config"
STATE="$W/ssh_config.state.json"
BACKUP="$W/ssh_config.backup"
INC="$W/ssh_config"
# The line, as Silo would write it: with ~ when the file is under the home folder.
TILDE="~"
# (Not on Linux: there ssh expands ~ to the passwd entry's home, not the analogue's $HOME.)
case "$INC" in
  "$HOME"/*) if [ "$LINUX" = 0 ]; then INC_REF="$TILDE/${INC#"$HOME"/}"; else INC_REF="$INC"; fi ;;
  *) INC_REF="$INC" ;;
esac
case "$INC_REF" in *" "*) INC_REF="\"$INC_REF\"" ;; esac
LINE="Include $INC_REF"
# On Linux, ssh reads the passwd entry's home, not $HOME: the analogue passes the file with -F.
if [ "$LINUX" = 1 ]; then SSHF="-F $CONFIG"; else SSHF=""; fi

restored=0
restore_config() {
  [ "$restored" = 1 ] && return 0
  [ -f "$STATE" ] || return 0
  local r
  r="$(python3 -I "$KIT_ROOT/py/08-sshconfig.py" restore --state "$STATE")"
  restored=1
  kit_fact ssh_config_restore "$r"
  if echo "$r" | grep -q '"verified": true'; then
    if [ -f "$BACKUP" ] && [ -f "$CONFIG" ] && cmp -s "$BACKUP" "$CONFIG"; then
      kit_log "$CONFIG is back, byte for byte (cmp)"
    elif [ ! -f "$BACKUP" ] && [ ! -e "$CONFIG" ]; then
      kit_log "$CONFIG did not exist before and does not now"
    fi
    rm -f "$STATE"
  else
    kit_log "WARNING: ~/.ssh/config is NOT what it was. The original is $BACKUP; see the fact ssh_config_restore."
  fi
}

SSHD_PID=""
cleanup() {
  restore_config
  [ -n "$SSHD_PID" ] && kill "$SSHD_PID" 2>/dev/null
  if [ "$LINUX" = 0 ]; then guest_cleanup; fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

# A run that was cut off (power, kill -9) left its line: take it out before anything else.
if [ -f "$STATE" ]; then
  kit_note "a previous run left its state; restoring ~/.ssh/config first"
  restore_config; restored=0
fi

# --- the computer, and what reaches its sshd --------------------------------------------------
if [ "$LINUX" = 1 ]; then
  [ "$(id -u)" = 0 ] || kit_skip "the Linux analogue needs root for a throwaway sshd"
  port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
  SSHD_PID="$(bash "$KIT_ROOT/linux/sshd-up.sh" "$W/sshd" 127.0.0.1 "$port")" || kit_skip "no sshd here"
  cat "$(_vm_key).pub" >> "$W/sshd/authorized_keys"
  TARGET="tcp:127.0.0.1:$port"; G_USER=root
  i=0; while ! python3 -c "import socket; socket.create_connection(('127.0.0.1', $port), 1)" 2>/dev/null && [ "$i" -lt 30 ]; do sleep 0.2; i=$((i + 1)); done
  hostkey="$(cut -d' ' -f1,2 "$W/sshd/host_ed25519.pub")"
else
  kit_need_mac
  . "$KIT_ROOT/lib/guest.sh"
  APPS=""
  for app in /Applications/Claude.app /Applications/Codex.app /Applications/ChatGPT.app; do
    if [ -d "$app" ]; then
      APPS="$APPS $(basename "$app" .app)"
      kit_fact "app.$(basename "$app" .app)" "$(defaults read "$app/Contents/Info" CFBundleShortVersionString 2>/dev/null)"
    fi
  done
  [ -n "$APPS" ] || kit_skip "neither Claude desktop nor the Codex or ChatGPT app is in /Applications"
  kit_fact ssh_version "$(ssh -V 2>&1)"
  kit_confirm "Start one macOS computer (Apple's NAT) for about 20 minutes, install the Codex command line in it (Codex's own installer, about 100 MB), and later let Claude desktop install its helper there (about 1 GB, inside the computer)."
  guest_up n08
  ip="$(vm_ip "$GB" 30)" || kit_fail "no address for the computer"
  TARGET="tcp:$ip:22"; G_USER="$VM_USER"
  hostkey="$(guest_sh 'cut -d" " -f1,2 /etc/ssh/ssh_host_ed25519_key.pub')"
  v="$(guest_install_codex)"; kit_fact codex_inside "${v:-not installed}"
  # The Codex app starts `codex` in the user's login shell (zsh): make sure that shell finds it.
  guest_sh 'grep -q silokit-path ~/.zprofile 2>/dev/null || echo "export PATH=\"\$HOME/.local/bin:\$PATH\" # silokit-path" >> ~/.zprofile; zsh -lc "command -v codex"' > "$W/codex-path.txt" 2>&1
  kit_fact codex_on_login_path "$(cat "$W/codex-path.txt")"
fi
[ -n "$hostkey" ] || kit_fail "could not read the computer's host key"
echo "$HOSTN $hostkey" > "$W/known_hosts"
: > "$W/proxy.log"

# --- Silo's file: two entries, the fd-passing one and a relaying fallback -----------------------
q() { printf '"%s"' "$1"; }
{
  for mode in fdpass relay; do
    if [ "$mode" = fdpass ]; then name="$HOSTN"; else name="$HOSTN-relay"; fi
    echo "Host $name"
    echo "  HostName $name"
    echo "  User $G_USER"
    if [ "$mode" = fdpass ]; then
      echo "  ProxyCommand $(q "$PROXY") --fdpass --log $(q "$W/proxy.log") $TARGET"
      echo "  ProxyUseFdpass yes"
    else
      echo "  ProxyCommand $(q "$PROXY") --log $(q "$W/proxy.log") $TARGET"
    fi
    echo "  IdentityFile $(q "$(_vm_key)")"
    echo "  IdentitiesOnly yes"
    echo "  HostKeyAlias $HOSTN"
    echo "  UserKnownHostsFile $(q "$W/known_hosts")"
    echo "  StrictHostKeyChecking yes"
    echo "  ForwardAgent no"
    echo ""
  done
} > "$INC"
chmod 600 "$INC"
kit_attach "$INC"

if [ "$LINUX" = 1 ]; then
  mkdir -p "$HOME/.ssh"
else
  kit_confirm "This adds one line, '$LINE', at the top of ~/.ssh/config$([ -L "$CONFIG" ] && echo " (a link: the file it points to, $(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$CONFIG"), is the one changed)"). A copy of the file is kept in $BACKUP. The line is taken out when the test ends, however it ends (or at the start of the next run if the Mac stops first), and the file is checked to be byte for byte what it was."
fi
r="$(python3 -I "$KIT_ROOT/py/08-sshconfig.py" install --config "$CONFIG" --line "$LINE" --state "$STATE" --backup "$BACKUP")" ||
  kit_fail "could not add the Include line: $r"
kit_fact ssh_config_install "$r"

# --- what ssh itself makes of it ---------------------------------------------------------------
# shellcheck disable=SC2086
ssh $SSHF -G "$HOSTN" > "$W/ssh-G.txt" 2>&1
kit_attach "$W/ssh-G.txt"
kit_fact ssh_G "$(grep -iE '^(user|proxycommand|proxyusefdpass|identitiesonly|forwardagent|hostkeyalias|stricthostkeychecking) ' "$W/ssh-G.txt" | tr '\n' ';')"
kit_fact ssh_G_identityfiles "$(grep -i '^identityfile ' "$W/ssh-G.txt" | tr '\n' ';')"
fwd="$(grep -iE '^(localforward|remoteforward|dynamicforward|permitremoteopen) ' "$W/ssh-G.txt" | grep -vi '^permitremoteopen any$' | tr '\n' ';')"
kit_fact ssh_G_forwards_from_your_config "${fwd:-none}"
grep -qi '^proxyusefdpass yes' "$W/ssh-G.txt" || kit_note "ssh -G does not show ProxyUseFdpass yes for $HOSTN: the Include line is not applied as expected"

auto_ok=1
for h in "$HOSTN" "$HOSTN-relay"; do
  # shellcheck disable=SC2086
  if out="$(ssh $SSHF -o BatchMode=yes "$h" 'echo silo-kit-ok' 2>&1 < /dev/null)" && [ "$out" = silo-kit-ok ]; then
    kit_fact "ssh_cli.$h" ok
  else
    kit_fact "ssh_cli.$h" "failed: $(echo "$out" | tail -2 | tr '\n' ' ')"; auto_ok=0
  fi
done
kit_fact proxy_log_cli "$(python3 -c '
import json, sys
for l in open(sys.argv[1]):
    e = json.loads(l)
    print(e["mode"], e["ok"], " < ".join(p["comm"] for p in e["chain"]))' "$W/proxy.log" | tr '\n' ';')"

# apps_since MARK: which apps' ssh invoked the proxy since line MARK of proxy.log, by mode.
apps_since() {
  python3 - "$W/proxy.log" "$1" <<'EOF'
import json, sys
seen = set()
for i, l in enumerate(open(sys.argv[1])):
    if i < int(sys.argv[2]): continue
    try: e = json.loads(l)
    except ValueError: continue
    app = ""
    for p in e.get("chain", []):
        if ".app/" in (p.get("path") or ""):
            app = p["path"].split(".app/")[0].rsplit("/", 1)[-1]
            break
    seen.add("%s:%s:%s" % (app or "no-app", e.get("mode"), "ok" if e.get("ok") else "failed"))
print(" ".join(sorted(seen)) or "none")
EOF
}
mark() { wc -l < "$W/proxy.log" | tr -d ' '; }

CODEX_LIST=unknown; CODEX_FD=untried; CODEX_RELAY=untried
CLAUDE_FD=untried; CLAUDE_RELAY=untried; CLAUDE_Q=unknown

if [ "$LINUX" = 0 ] && [ "$auto_ok" = 1 ]; then
  case "$APPS" in *Codex*|*ChatGPT*)
    app="$(echo "$APPS" | grep -o 'Codex' || echo ChatGPT)"
    kit_human "In the $app app: quit it first if it is open (Cmd-Q) and open it again, so it reads ~/.ssh/config afresh. Then open Settings > Connections and look at the SSH hosts. Do not add any host by hand."
    CODEX_LIST="$(kit_ask_yn "Does $HOSTN appear there by itself (it comes only from the included file)?")"
    kit_fact codex.lists_included_host "$CODEX_LIST"
    kit_fact codex.lists_relay_host "$(kit_ask_yn "Does $HOSTN-relay appear too?")"
    m0="$(mark)"
    kit_human "Still in Settings > Connections: add or enable $HOSTN (if it is not listed, add it by typing $HOSTN), and choose a remote project folder such as /Users/$G_USER. Wait up to a minute for it to connect. It may say Codex is not signed in on the host: that still counts as connected."
    CODEX_FD="$(kit_ask_yn "Did the app connect to $HOSTN (it shows the folder, a project, or a sign-in step for the remote Codex)?")"
    kit_fact codex.connect_fdpass "$CODEX_FD (proxy calls: $(apps_since "$m0"))"
    if [ "$CODEX_FD" != yes ]; then
      m0="$(mark)"
      kit_human "Now the same with $HOSTN-relay (the ProxyCommand that relays bytes instead of passing the socket)."
      CODEX_RELAY="$(kit_ask_yn "Did the app connect to $HOSTN-relay?")"
      kit_fact codex.connect_relay "$CODEX_RELAY (proxy calls: $(apps_since "$m0"))"
    fi
    kit_fact codex.inside "$(guest_sh 'pgrep -fl codex | head -3; ls -d ~/.codex 2>/dev/null' 2>/dev/null | tr '\n' ' ')"
    ;;
  esac
  case "$APPS" in *Claude*)
    m0="$(mark)"
    kit_human "In Claude (the desktop app), Code tab: open the environment selector under the prompt box > New environment > SSH tab > Host > Add SSH connection. SSH host: $HOSTN. Leave the port and key empty. Add it, choose the folder /Users/$G_USER, select Create environment, then send the message: run uname -a. Desktop may spend a few minutes installing its helper on the computer first. Answer any question it asks as you see fit, and note it."
    CLAUDE_Q="$(kit_ask_yn "Did Desktop ask 'Connect to $HOSTN for the first time?' or show a host key question, although Silo's known-hosts file already had the key?")"
    kit_fact claude.host_key_question "$CLAUDE_Q"
    kit_fact claude.known_hosts_message "$(kit_ask_yn "Did Desktop say anything about UserKnownHostsFile or a host key not trusted?")"
    CLAUDE_FD="$(kit_ask_yn "Did the session connect and run the command on the computer?")"
    kit_fact claude.connect_fdpass "$CLAUDE_FD (proxy calls: $(apps_since "$m0"))"
    if [ "$CLAUDE_FD" != yes ]; then
      m0="$(mark)"
      kit_human "Now add another SSH connection with SSH host $HOSTN-relay and try the same message."
      CLAUDE_RELAY="$(kit_ask_yn "Did that session connect and run the command?")"
      kit_fact claude.connect_relay "$CLAUDE_RELAY (proxy calls: $(apps_since "$m0"))"
    fi
    kit_fact claude.helper_inside "$(guest_sh 'ls -d ~/.claude/remote 2>/dev/null && du -sh ~/.claude/remote 2>/dev/null | cut -f1' 2>/dev/null | tr '\n' ' ')"
    ;;
  esac
  kit_note "The computer is deleted at the end: remove the $HOSTN entries you added in the apps, or they keep trying to reach it."
fi
kit_attach "$W/proxy.log"

restore_config
python3 -I "$KIT_ROOT/py/08-sshconfig.py" check --config "$CONFIG" --line "$LINE" && kit_note "WARNING: the Include line is still first in ~/.ssh/config"

# --- the outcome -------------------------------------------------------------------------------
summary="Codex lists the included host: $CODEX_LIST; Codex through fd passing: $CODEX_FD, relaying: $CODEX_RELAY; Claude desktop through fd passing: $CLAUDE_FD, relaying: $CLAUDE_RELAY, asked about the host key anyway: $CLAUDE_Q; ssh in Terminal through both: $([ "$auto_ok" = 1 ] && echo ok || echo failed); forwards your own config adds to every host: ${fwd:-none}"
if [ "$LINUX" = 1 ]; then
  if [ "$auto_ok" = 1 ]; then
    kit_outcome inconclusive "Linux analogue, not a Mac result: ssh through the Included entry worked with ProxyUseFdpass and with the relay, and ~/.ssh/config was restored; no app was tried. $summary"
  else
    kit_outcome inconclusive "Linux analogue: ssh through the Included entry failed (see the facts). $summary"
  fi
elif [ "$auto_ok" != 1 ]; then
  kit_outcome inconclusive "ssh in Terminal did not get through the Included entry, so no app was tried; see ssh_cli and ssh-G.txt. $summary"
elif [ "$CODEX_LIST" = no ]; then
  kit_outcome codex-ignores-include "$summary"
elif { [ "$CODEX_FD" = no ] && [ "$CODEX_RELAY" = yes ]; } || { [ "$CLAUDE_FD" = no ] && [ "$CLAUDE_RELAY" = yes ]; }; then
  kit_outcome fdpass-refused "$summary"
elif { [ "$CODEX_FD" = no ] && [ "$CODEX_RELAY" = no ]; } || { [ "$CLAUDE_FD" = no ] && [ "$CLAUDE_RELAY" = no ]; }; then
  kit_outcome proxycommand-refused "$summary"
elif { [ "$CODEX_FD" = yes ] || [ "$CLAUDE_FD" = yes ]; } && [ "$CODEX_FD" != no ] && [ "$CLAUDE_FD" != no ]; then
  kit_outcome include-and-fdpass-work "$summary"
else
  kit_outcome inconclusive "$summary"
fi
