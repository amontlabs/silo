#!/bin/bash
# id: 21
# title: Do Claude Code and Codex trust the computer's keychain authorities
# tier: vm
# time: about 25 minutes; a person for one approval if macOS asks for it
# needs: the installed computer of test 00, macOS 27 on the Mac, network (downloads Claude Code and Codex inside the computer, about 300 MB)
# step: 2
# decides: reading-each-agent-program on macOS (how model traffic is read in a macOS computer, second build)
# blocks: option A of the first build (#373): the first run puts Silo's authority in main, and this says where (keychain, or each program's own setting)
# question: Inside a macOS computer, does each agent program accept a TLS certificate from an authority trusted in the System keychain, and does it accept one given only in its own setting (NODE_EXTRA_CA_CERTS, CODEX_CA_CERTIFICATE, SSL_CERT_FILE)?
# outcome both-trust-keychain: Claude Code and Codex both accepted the test authority from the System
#   keychain, and refused it when it was nowhere: one trusted authority in the computer's keychain is
#   enough to read both programs' traffic; no per-program setting is needed.
# outcome claude-only-keychain: Claude Code trusts the keychain, Codex does not: Codex needs its own
#   setting (the facts say whether CODEX_CA_CERTIFICATE or SSL_CERT_FILE worked), which Silo must set in
#   the environment of every Codex it starts, or Codex's traffic is not readable.
# outcome codex-only-keychain: Codex trusts the keychain, Claude Code does not: Claude Code needs
#   NODE_EXTRA_CA_CERTS (or CLAUDE_CODE_CERT_STORE) in its environment.
# outcome neither-keychain: Neither program accepted the keychain's authority: each needs its own
#   setting; the facts say which settings worked.
# outcome keychain-needs-a-person: macOS would not trust the authority in the computer's keychain
#   without someone approving it on the computer's screen, and nobody did: Silo cannot set up the
#   keychain way by itself (only a person, a profile from device management, or SIP off can); the
#   programs' own settings (facts) are then the only way, set in each program's environment.
# outcome inconclusive: The control (authority nowhere) was not refused, or no program connected to
#   the test server (a proxy, IPv6, or a DNS cache may have sent it elsewhere): read the facts and
#   the attached server logs.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"

PORT=443
if guest_local; then
  # Linux analogue: curl stands in for both programs, SSL_CERT_FILE for "the keychain"; the test
  # server, the certificates and the verdicts are the same code as on a Mac.
  PORT=18443
  kit_note "Linux run: curl plays both programs, on port $PORT; nothing is trusted system-wide"
else
  kit_need_mac 27
  kit_need_silovm; kit_need_base_vm
  kit_need_cmd openssl "it ships with macOS"
  kit_confirm "This starts one macOS computer with its screen in a window (a clone of the installed one, deleted at the end), downloads Claude Code and Codex into it (about 300 MB), and trusts a test authority inside that computer only, for about 25 minutes."
fi

tls="$KIT_WORK/tls"
rm -rf "$tls"
guest_make_test_ca "$tls" || kit_fail "openssl could not make the test certificates"

trap guest_cleanup EXIT
if guest_local; then guest_up trust; else guest_up trust --gui; fi

if guest_local; then
  CLAUDE_V="curl (Linux stand-in)"; CODEX_V="curl (Linux stand-in)"
else
  CLAUDE_V="$(guest_install_claude)"; CODEX_V="$(guest_install_codex)"
  [ -n "$CLAUDE_V" ] || kit_note "Claude Code did not install; its runs below will connect to nothing"
  [ -n "$CODEX_V" ] || kit_note "Codex did not install; its runs below will connect to nothing"
fi
kit_fact claude_version "${CLAUDE_V:-not installed}"
kit_fact codex_version "${CODEX_V:-not installed}"

for f in ca.crt leaf.crt leaf.key; do guest_put "$tls/$f" "silokit-tls/$f" || kit_fail "could not copy $f into the computer"; done

# The test server: openssl s_server (it sends the leaf only; the authority is the anchor a client
# trusts or not). With -state it logs each handshake: "read finished" when the
# client accepted the certificate, "alert read:fatal" when it refused it. TLS 1.2, because the
# LibreSSL of macOS logs every state there. -www answers at once, so the programs fail fast.
if guest_local; then
  srvhome="$GUEST_HOME/silokit-tls"
else
  srvhome="/Users/$VM_USER/silokit-tls"
  hosts=""
  for h in $GUEST_TLS_HOSTS; do hosts="$hosts
127.0.0.1 $h # silokit-21"; done
  guest_sudo "cp /etc/hosts /etc/hosts.silokit-21 && printf '%s\n' '$hosts' >> /etc/hosts && dscacheutil -flushcache; killall -HUP mDNSResponder 2>/dev/null; true"
  kit_fact hosts_lines "$(guest_sh 'grep -c silokit-21 /etc/hosts')"
fi
# Its stdin is a FIFO opened read-write, so it never sees an end of input.
guest_sudo "cd '$srvhome' && rm -f in.fifo && mkfifo in.fifo && { nohup openssl s_server -accept $PORT -tls1_2 -cert leaf.crt -key leaf.key -www -state 0<> in.fifo > server.log 2>&1 & echo \$! > server.pid; }; sleep 1; kill -0 \$(cat server.pid)" ||
  kit_fail "the test server did not start in the computer"
sleep 1

# run_case NAME PROGRAM "env assignments": run one program against the test server with a time
# limit, then read what the server logged during that run. Prints the verdict.
run_case() {
  local name="$1" prog="$2" envs="$3" cmd verdict
  case "$prog" in
    claude) cmd="limit 45 env $envs ANTHROPIC_API_KEY=sk-ant-api03-silokit-fake DISABLE_AUTOUPDATER=1 claude -p hi" ;;
    codex) cmd="limit 60 env $envs CODEX_API_KEY=sk-silokit-fake OPENAI_API_KEY=sk-silokit-fake codex exec --skip-git-repo-check hi" ;;
    curl) cmd="limit 20 env $envs curl -s --noproxy '*' --resolve api.anthropic.com:$PORT:127.0.0.1 https://api.anthropic.com:$PORT/v1/messages -o /dev/null" ;;
  esac
  guest_sudo "cd '$srvhome' && : > server.log.mark && wc -c < server.log > server.log.mark"
  guest_sh "$GUEST_LIMIT cd ~/silokit-tls && $cmd > run-$name.out 2>&1; echo \"exit \$?\" >> run-$name.out" >/dev/null 2>&1
  guest_sudo "cd '$srvhome' && tail -c +\$((\$(cat server.log.mark) + 1)) server.log > server-$name.log; chmod 644 server-$name.log"
  guest_get "silokit-tls/server-$name.log" "$KIT_WORK/server-$name.log"
  guest_get "silokit-tls/run-$name.out" "$KIT_WORK/run-$name.out"
  guest_redact < "$KIT_WORK/run-$name.out" > "$KIT_WORK/run-$name.txt"
  kit_attach "$KIT_WORK/server-$name.log"
  kit_attach "$KIT_WORK/run-$name.txt"
  verdict="$(guest_tls_verdict "$KIT_WORK/server-$name.log")"
  kit_fact "$name" "$verdict ($(tail -1 "$KIT_WORK/run-$name.txt"); last output: $(grep -v '^exit' "$KIT_WORK/run-$name.txt" | tail -1 | cut -c1-160))"
  echo "$verdict"
}

# keychain_add: trust the test authority in the computer's System keychain. Prints yes when the
# trust is in place. macOS asks for an administrator's approval on screen for this, even for root
# (Apple's forums: "no user interaction was possible" over SSH), so it is tried three ways: as root
# over SSH; with the right com.apple.trust-settings.admin allowed for one call and put back (the old
# way round); and, when a person is at the keyboard, by them in the computer's own window.
keychain_add() {
  if guest_local; then KEYCHAIN_ENV="CURL_CA_BUNDLE=$srvhome/ca.crt"; echo yes; return; fi
  local add="security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain '$srvhome/ca.crt'"
  local trusted="security verify-cert -c '$srvhome/leaf.crt' -p ssl -s api.anthropic.com >/dev/null 2>&1"
  local log="$KIT_WORK/add-trusted-cert.txt" way=none
  echo "# 1. as root over SSH" > "$log"
  guest_sudo "$GUEST_LIMIT limit 60 $add" >> "$log" 2>&1
  if guest_sh "$trusted"; then way=root-over-ssh; else
    echo "# 2. with com.apple.trust-settings.admin allowed for one call" >> "$log"
    guest_sudo "$GUEST_LIMIT security authorizationdb read com.apple.trust-settings.admin > /var/tmp/silokit-trust-right.plist 2>/dev/null; security authorizationdb write com.apple.trust-settings.admin allow; limit 60 $add; security authorizationdb write com.apple.trust-settings.admin < /var/tmp/silokit-trust-right.plist" >> "$log" 2>&1
    if guest_sh "$trusted"; then way=authorizationdb-allow; elif [ -t 0 ]; then
      kit_human "macOS refused to trust the test authority without a person. In the computer's window on your Mac:
  1. Open Terminal (Applications > Utilities).
  2. Run:  sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain ~/silokit-tls/ca.crt
     The sudo password, and the password of the dialog that follows, is in $KIT_SSH_DIR/password on your Mac.
  3. Approve the dialog (it changes trust settings inside that computer only)."
      if guest_sh "$trusted"; then way="a person in its window"; fi
    fi
  fi
  kit_attach "$log"
  kit_fact keychain_trust_installed_by "$way"
  if [ "$way" = none ]; then echo no; else echo yes; fi
}
KEYCHAIN_ENV=""
CA="$srvhome/ca.crt"

if guest_local; then
  c_none="$(run_case curl-none curl "")"
  c_env="$(run_case curl-env curl "CURL_CA_BUNDLE=$CA")"
  kc_in="$(keychain_add)"; KEYCHAIN_ENV="CURL_CA_BUNDLE=$CA"
  c_kc="$(run_case curl-keychain curl "$KEYCHAIN_ENV")"
  kit_measure linux_cases_as_expected "$( [ "$c_none" = refused ] && [ "$c_kc" = accepted ] && [ "$c_env" = accepted ] && echo 1 || echo 0)" bool "none refused, store accepted, env accepted"
  cl_none="$c_none"; cx_none="$c_none"; cl_kc="$c_kc"; cx_kc="$c_kc"; cl_env="$c_env"; cx_env="$c_env"; cx_ssl="$c_env"; cl_bundled="n/a"
else
  # 1. Control: the authority is nowhere. Both must refuse.
  cl_none="$(run_case claude-none claude "")"
  cx_none="$(run_case codex-none codex "")"
  # 2. The authority only in each program's own setting (before the keychain has it, so nothing
  #    has to be taken out of the keychain again).
  cl_env="$(run_case claude-env claude "NODE_EXTRA_CA_CERTS=$CA")"
  cx_env="$(run_case codex-env codex "CODEX_CA_CERTIFICATE=$CA")"
  cx_ssl="$(run_case codex-ssl-cert-file codex "SSL_CERT_FILE=$CA")"
  # 3. The authority trusted in the System keychain, nothing in the environment.
  kc_in="$(keychain_add)"
  cl_kc=not-run; cx_kc=not-run; cl_bundled=not-run
  if [ "$kc_in" = yes ]; then
    cl_kc="$(run_case claude-keychain claude "")"
    cx_kc="$(run_case codex-keychain codex "")"
    # 3b. Keychain, but Claude Code told to use only its bundled authorities: shows that its
    #     "system" store (the documented default is "bundled,system") is what accepted.
    cl_bundled="$(run_case claude-keychain-bundled-only claude "CLAUDE_CODE_CERT_STORE=bundled")"
  fi
fi
guest_sudo "kill \$(cat '$srvhome/server.pid'); [ -f /etc/hosts.silokit-21 ] && mv /etc/hosts.silokit-21 /etc/hosts; true" >/dev/null 2>&1

table="Claude Code: nowhere=$cl_none keychain=$cl_kc keychain+bundled-only=$cl_bundled NODE_EXTRA_CA_CERTS=$cl_env. Codex: nowhere=$cx_none keychain=$cx_kc CODEX_CA_CERTIFICATE=$cx_env SSL_CERT_FILE=$cx_ssl."
kit_fact summary "$table"

if [ "$kc_in" != yes ] && [ "$cl_none" = refused ]; then
  kit_outcome keychain-needs-a-person "macOS refused to trust an authority in the computer's keychain without a person at its screen (see add-trusted-cert.txt); the programs' own settings: $table"
elif [ "$cl_none" != refused ] || [ "$cx_none" != refused ]; then
  kit_outcome inconclusive "The control was not refused by both programs (Claude Code: $cl_none, Codex: $cx_none; none means no connection reached the test server). $table"
elif [ "$cl_kc" = accepted ] && [ "$cx_kc" = accepted ]; then
  kit_outcome both-trust-keychain "$table"
elif [ "$cl_kc" = accepted ]; then
  kit_outcome claude-only-keychain "$table"
elif [ "$cx_kc" = accepted ]; then
  kit_outcome codex-only-keychain "$table"
else
  kit_outcome neither-keychain "$table"
fi
