#!/bin/bash
# id: 28
# title: App Store Connect tools through a boundary that re-signs their tokens
# tier: guided
# time: about 30 minutes, with you at the keyboard for the key's details and perhaps one approval in the computer's window
# needs: the Xcode computer of test 23, macOS 27 on the Mac, Xcode on this Mac, an App Store Connect API key (.p8) on this Mac with its key ID and issuer ID (KIT_ASC_KEY, KIT_ASC_KEY_ID, KIT_ASC_ISSUER, or asked), network; for xcodebuild, KIT_ASC_PROJECT and KIT_ASC_SCHEME (a project of yours, copied into the computer)
# step: later
# decides: signing-keys-re-signed, apple-keys-in-a-computer (whether an App Store Connect key can stay out of every computer)
# question: With the real App Store Connect key kept on the Mac and a placeholder key in the computer, do notarytool, altool and xcodebuild -allowProvisioningUpdates work through a boundary that ends TLS and signs each token again with the real key, also when each GET token is scoped to its one request, and which hosts, audiences and token lifetimes do they use?
# outcome works-scoped: Every tool tried worked through the boundary, also with each GET token scoped
#   to its one request: signing-keys-re-signed can be built as proposed, with the narrowing; the
#   hosts and audiences it must handle are in the facts.
# outcome works-unscoped-only: Every tool worked when its token was re-signed, but a GET token scoped
#   to its request was refused by Apple: the boundary re-signs without a scope, and its own rule on
#   method and path is the only narrowing.
# outcome some-tools: Some tools worked and others did not; the facts name, for each, the hosts it
#   called, the audience and lifetime of its token and why it failed (a host outside the list, a
#   longer lifetime, a refused certificate). signing-keys-re-signed handles the ones that worked
#   first, and its notes say what the others need.
# outcome certificate-refused: The tools refused the boundary's certificate even though the
#   computer's System keychain trusts its authority (they pin Apple's): no boundary can read or
#   re-sign their traffic, and the key must be handled another way (Apple's cloud signing, or the
#   tool run on the Mac).
# outcome none-work: The boundary saw and re-signed the tools' tokens but none worked; the statuses
#   and errors are in the facts. Read them before changing signing-keys-re-signed.
# outcome inconclusive: The key did not work from this Mac itself, the computer's requests never
#   reached the boundary, or its authority could not be trusted; the sentence says which.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"
. "$KIT_ROOT/lib/ios.sh"

BOUNDARY="$KIT_ROOT/py/28-asc-boundary.py"
ASC_HOSTS="api.appstoreconnect.apple.com appstoreconnect.apple.com contentdelivery.itunes.apple.com developerservices2.apple.com api.developer.apple.com"
W="$KIT_WORK"
PH_KID=SILOKIT001

if guest_local; then
  ios_fake || kit_skip "needs a Mac: the tools are Xcode's"
  kit_note "Linux run: the boundary against a fake App Store Connect on 127.0.0.1 (py/28-asc-boundary.py selftest)"
  python3 -I "$BOUNDARY" selftest > "$W/selftest.txt" 2>&1; rc=$?
  kit_attach "$W/selftest.txt"
  kit_measure linux_boundary_ok "$( [ "$rc" = 0 ] && echo 1 || echo 0)" bool "re-signs, scopes, refuses other hosts, keys, lifetimes and audiences, logs refused handshakes"
  kit_outcome inconclusive "Linux run: only the boundary's logic ran, against a fake App Store Connect; the tools need a Mac."
  exit 0
fi

kit_need_mac 27
kit_need_silovm
ios_need_xcode_vm
kit_need_cmd openssl "it ships with macOS"
kit_need_cmd xcrun "install Xcode"

# ask "question": a value only the person can give. Empty without a terminal.
ask() {
  [ -t 0 ] || return 0
  printf '[%s] %s ' "$KIT_ID" "$1" >&2
  local a; read -r a; echo "$a"
}
KEY="${KIT_ASC_KEY:-}"
[ -n "$KEY" ] || KEY="$(ask 'Path of your App Store Connect API key (AuthKey_XXXXXXXXXX.p8). It is only read, on this Mac, and never copied into the computer:')"
# shellcheck disable=SC2088  # a "~/" the person typed, made a path
case "$KEY" in "~/"*) KEY="$HOME/${KEY#\~/}" ;; esac
[ -f "$KEY" ] || kit_skip "needs an App Store Connect API key on this Mac: set KIT_ASC_KEY, KIT_ASC_KEY_ID and KIT_ASC_ISSUER, or run it at the keyboard"
KID="${KIT_ASC_KEY_ID:-$(basename "$KEY" | sed -n 's/^AuthKey_\(.*\)\.p8$/\1/p')}"
[ -n "$KID" ] || KID="$(ask 'Its key ID:')"
[ -n "$KID" ] || kit_skip "needs the key's ID (KIT_ASC_KEY_ID)"
if [ "${KIT_ASC_ISSUER+set}" = set ]; then ISS="$KIT_ASC_ISSUER"; else ISS="$(ask 'Its issuer ID (Users and Access > Integrations; press Enter for an individual key):')"; fi
kit_fact key_kind "$([ -n "$ISS" ] && echo team || echo individual)"

# 1. Control: the real key, from this Mac, with nothing in between. A key that fails here says
# nothing about the boundary.
mac_limit() { perl -e 'alarm shift; exec @ARGV' "$@"; }
ctl="$W/control"
rm -rf "$ctl"; mkdir -p "$ctl/private_keys"
ln -s "$KEY" "$ctl/private_keys/AuthKey_$KID.p8"
( cd "$ctl" && mac_limit 120 xcrun notarytool history --key "$KEY" --key-id "$KID" ${ISS:+--issuer "$ISS"} --output-format json ) > "$ctl/notarytool.out" 2>&1; c_not=$?
( cd "$ctl" && API_PRIVATE_KEYS_DIR="$ctl/private_keys" mac_limit 120 xcrun altool --list-apps --apiKey "$KID" ${ISS:+--apiIssuer "$ISS"} --output-format json ) > "$ctl/altool.out" 2>&1; c_alt=$?
rm -rf "$ctl"
kit_fact control_on_mac "notarytool exit $c_not, altool exit $c_alt"
if [ "$c_not" != 0 ] && [ "$c_alt" != 0 ]; then
  kit_outcome inconclusive "the key did not work from this Mac itself (notarytool exit $c_not, altool exit $c_alt): check its ID, issuer and role, then run 28 again"
  exit 0
fi

kit_confirm "This starts a clone of the Xcode computer with its screen in a window for about 30 minutes, runs a boundary on 127.0.0.1 of this Mac that reads your key and signs the computer's App Store Connect requests with it (read-only calls: notarization history, the list of apps${KIT_ASC_PROJECT:+, and a build of $KIT_ASC_PROJECT that may create a development certificate and profile}). The key never enters the computer."

# 2. The boundary's certificate, for Apple's host names, and the placeholder key.
tls="$W/tls"; rm -rf "$tls"; mkdir -p "$tls"
sans=""; for h in $ASC_HOSTS; do sans="$sans${sans:+, }DNS:$h"; done
(
  cd "$tls" || exit 1
  printf '[req]\ndistinguished_name = dn\nprompt = no\n[dn]\nCN = Silo Kit Test CA\n[v3_ca]\nbasicConstraints = critical, CA:TRUE\nkeyUsage = critical, keyCertSign, cRLSign\nsubjectKeyIdentifier = hash\n' > ca.cnf
  printf '[req]\ndistinguished_name = dn\nprompt = no\n[dn]\nCN = api.appstoreconnect.apple.com\n[v3_leaf]\nbasicConstraints = critical, CA:FALSE\nkeyUsage = critical, digitalSignature, keyEncipherment\nextendedKeyUsage = serverAuth\nsubjectAltName = %s\nauthorityKeyIdentifier = keyid\n' "$sans" > leaf.cnf
  openssl req -x509 -newkey rsa:2048 -nodes -keyout ca.key -out ca.crt -days 30 -config ca.cnf -extensions v3_ca 2>/dev/null &&
    openssl req -new -newkey rsa:2048 -nodes -keyout leaf.key -out leaf.csr -config leaf.cnf 2>/dev/null &&
    openssl x509 -req -in leaf.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out leaf.crt -days 30 -extfile leaf.cnf -extensions v3_leaf 2>/dev/null &&
    cat leaf.crt ca.crt > chain.crt &&
    openssl ecparam -name prime256v1 -genkey -noout -out ph.ec 2>/dev/null &&
    openssl pkcs8 -topk8 -nocrypt -in ph.ec -out ph.p8
) || kit_fail "openssl could not make the boundary's certificate or the placeholder key"
PH_ISS=""; [ -n "$ISS" ] && PH_ISS="$(python3 -c 'import uuid; print(uuid.uuid4())')"

log="$W/boundary.jsonl"; : > "$log"; echo plain > "$W/mode"; rm -f "$W/port"
python3 -I "$BOUNDARY" serve --port-file "$W/port" --cert "$tls/chain.crt" --key "$tls/leaf.key" \
  --real-key "$KEY" --real-kid "$KID" --real-iss "$ISS" --ph-key "$tls/ph.p8" --ph-kid "$PH_KID" --ph-iss "$PH_ISS" \
  --log "$log" --mode-file "$W/mode" --hosts "$(echo "$ASC_HOSTS" | tr ' ' ',')" --max-seconds 7200 > "$W/boundary.err" 2>&1 &
BPID=$!
TPID=""
trap 'kill $BPID $TPID 2>/dev/null; guest_cleanup' EXIT
i=0; while [ ! -s "$W/port" ] && [ "$i" -lt 20 ]; do sleep 1; i=$((i + 1)); done
[ -s "$W/port" ] || { kit_attach "$W/boundary.err"; kit_fail "the boundary did not start"; }
PORT="$(cat "$W/port")"

# 3. The computer: its requests to Apple's hosts go to 127.0.0.1:443, which a relay run as root
# hands to a port forwarded over SSH to the boundary on this Mac.
ios_up asc --gui
ip="$(vm_ip "$GB" 30)" || kit_fail "no address for the computer"
# shellcheck disable=SC2046
ssh $(_vm_ssh_opts) -o BatchMode=yes -o ExitOnForwardFailure=yes -i "$(_vm_key)" -N \
  -R "127.0.0.1:18443:127.0.0.1:$PORT" "$VM_USER@$ip" < /dev/null > "$W/tunnel.log" 2>&1 &
TPID=$!
cat > "$W/relay.py" <<'EOF'
import socket, threading
def pump(a, b):
    try:
        while True:
            d = a.recv(65536)
            if not d:
                break
            b.sendall(d)
    except OSError:
        pass
    try:
        b.shutdown(socket.SHUT_WR)
    except OSError:
        pass
def serve(fam, addr):
    l = socket.socket(fam, socket.SOCK_STREAM)
    l.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    l.bind((addr, 443))
    l.listen(64)
    while True:
        c, _ = l.accept()
        try:
            u = socket.create_connection(("127.0.0.1", 18443))
        except OSError:
            c.close()
            continue
        threading.Thread(target=pump, args=(c, u), daemon=True).start()
        threading.Thread(target=pump, args=(u, c), daemon=True).start()
threading.Thread(target=serve, args=(socket.AF_INET6, "::1"), daemon=True).start()
serve(socket.AF_INET, "127.0.0.1")
EOF
guest_put "$W/relay.py" "silokit/relay.py" || kit_fail "could not copy relay.py into the computer"
for f in ca.crt leaf.crt; do guest_put "$tls/$f" "silokit/tls/$f" || kit_fail "could not copy $f into the computer"; done
guest_put "$tls/ph.p8" "private_keys/AuthKey_$PH_KID.p8" || kit_fail "could not copy the placeholder key into the computer"
hosts=""
for h in $ASC_HOSTS; do hosts="$hosts
127.0.0.1 $h # silokit-28
::1 $h # silokit-28"; done
guest_sudo "cp /etc/hosts /etc/hosts.silokit-28 && printf '%s\n' '$hosts' >> /etc/hosts && dscacheutil -flushcache; killall -HUP mDNSResponder 2>/dev/null
nohup /usr/bin/python3 /Users/$VM_USER/silokit/relay.py > /var/tmp/silokit-relay.log 2>&1 < /dev/null &
sleep 2; true" >/dev/null 2>&1
sleep 2
path_check="$(guest_sh "curl -sk -o /dev/null -w '%{http_code}' --max-time 30 https://api.appstoreconnect.apple.com/v1/apps")"
kit_fact path_check "$path_check (a status from Apple through the boundary, unsigned)"
case "$path_check" in
  ""|000) kit_attach "$W/tunnel.log"; kit_outcome inconclusive "the computer's requests to App Store Connect did not reach the boundary (curl said ${path_check:-nothing})"; exit 0 ;;
esac

trust="$(ios_trust_ca "/Users/$VM_USER/silokit/tls/ca.crt" "/Users/$VM_USER/silokit/tls/leaf.crt" api.appstoreconnect.apple.com "$W/trust.log")"
kit_attach "$W/trust.log"
kit_fact authority_trusted_by "$trust"
[ "$trust" != none ] || { kit_outcome inconclusive "the boundary's test authority could not be trusted in the computer's System keychain (see trust.log and test 21)"; exit 0; }

# 4. The tools. run_tool NAME MODE DIR "command": run it in the computer from ~/DIR, with the
# boundary in MODE (plain or scoped); keep its last lines and what the boundary saw. Prints ok,
# tls-refused, not-seen (no request reached the boundary) or failed.
run_tool() {
  local name="$1" mode="$2" dir="$3" cmd="$4" from rc sum v
  echo "$mode" > "$W/mode"
  from="$(wc -l < "$log" | tr -d ' ')"
  rc="$(guest_sh "$GUEST_LIMIT cd \$HOME/$dir && limit ${5:-300} $cmd > \$HOME/silokit/$name-$mode.out 2>&1; echo \$?" | tail -1)"
  guest_get "silokit/$name-$mode.out" "$W/$name-$mode.raw" 2>/dev/null
  tail -8 "$W/$name-$mode.raw" 2>/dev/null | guest_redact > "$W/$name-$mode.txt"; rm -f "$W/$name-$mode.raw"
  kit_attach "$W/$name-$mode.txt"
  sum="$(python3 -I "$BOUNDARY" summarize "$log" "$from")"
  v="$(python3 -c 'import json,sys
s=json.loads(sys.argv[1]); rc=sys.argv[2]
if rc == "0" and s["resigned_ok"] > 0: print("ok")
elif s["requests"] == 0 and s["handshakes_refused"] > 0: print("tls-refused")
elif s["requests"] == 0: print("not-seen")
else: print("failed")' "$sum" "$rc")"
  kit_fact "$name-$mode" "$v (exit $rc): $sum"
  echo "$v"
}

NOT="xcrun notarytool history --key \$HOME/private_keys/AuthKey_$PH_KID.p8 --key-id $PH_KID ${PH_ISS:+--issuer $PH_ISS} --output-format json"
ALT="env API_PRIVATE_KEYS_DIR=\$HOME/private_keys xcrun altool --list-apps --apiKey $PH_KID ${PH_ISS:+--apiIssuer $PH_ISS} --output-format json"
tools=""; results=""
try_tool() { # try_tool NAME DIR "command" [limit]: plain, then scoped when plain worked
  local p s=not-run
  p="$(run_tool "$1" plain "$2" "$3" "${4:-300}")"
  [ "$p" = ok ] && s="$(run_tool "$1" scoped "$2" "$3" "${4:-300}")"
  tools="$tools $1"; results="$results $1:$p:$s"
}
if [ "$c_not" = 0 ]; then try_tool notarytool . "$NOT"; else kit_note "notarytool skipped: the key failed with it on this Mac too"; fi
if [ "$c_alt" = 0 ]; then try_tool altool . "$ALT"; else kit_note "altool skipped: the key failed with it on this Mac too"; fi
if [ -n "${KIT_ASC_PROJECT:-}" ] && [ -d "$KIT_ASC_PROJECT" ] && [ -n "${KIT_ASC_SCHEME:-}" ]; then
  pj="$(basename "$KIT_ASC_PROJECT")"
  ( cd "$(dirname "$KIT_ASC_PROJECT")" && tar -cf - "$pj" ) |
    vm_ssh "$GB" "$VM_USER" "mkdir -p \$HOME/silokit/project && tar -C \$HOME/silokit/project -xf -" > "$W/project-copy.log" 2>&1
  try_tool xcodebuild "silokit/project/$pj" "xcodebuild -scheme '$KIT_ASC_SCHEME' -destination generic/platform=iOS -derivedDataPath \$HOME/silokit/dd -allowProvisioningUpdates -authenticationKeyPath \$HOME/private_keys/AuthKey_$PH_KID.p8 -authenticationKeyID $PH_KID ${PH_ISS:+-authenticationKeyIssuerID $PH_ISS} build" 1800
else
  kit_note "xcodebuild -allowProvisioningUpdates skipped: set KIT_ASC_PROJECT (a folder holding your Xcode project) and KIT_ASC_SCHEME to try it"
fi
kit_attach "$log"
all="$(python3 -I "$BOUNDARY" summarize "$log" 0)"
kit_fact boundary_saw "$all"
kit_fact results "$results"

[ -n "$tools" ] || { kit_outcome inconclusive "no tool could be tried"; exit 0; }
n_ok=0; n_scoped=0; n_tls=0; n_tools=0
for r in $results; do
  n_tools=$((n_tools + 1))
  p="$(echo "$r" | cut -d: -f2)"; s="$(echo "$r" | cut -d: -f3)"
  [ "$p" = ok ] && n_ok=$((n_ok + 1))
  [ "$s" = ok ] && n_scoped=$((n_scoped + 1))
  [ "$p" = tls-refused ] && n_tls=$((n_tls + 1))
done
sentence="$results. The boundary saw: $all"
if [ "$n_ok" = "$n_tools" ] && [ "$n_scoped" = "$n_tools" ]; then kit_outcome works-scoped "$sentence"
elif [ "$n_ok" = "$n_tools" ]; then kit_outcome works-unscoped-only "$sentence"
elif [ "$n_ok" -gt 0 ]; then kit_outcome some-tools "$sentence"
elif [ "$n_tls" -gt 0 ]; then kit_outcome certificate-refused "$sentence"
else kit_outcome none-work "$sentence"
fi
