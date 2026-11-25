#!/bin/bash
# id: 27
# title: Which certificates an app in a simulator trusts
# tier: vm
# time: about 20 minutes
# needs: the Xcode computer of test 23 (with its user on the screen if test 24 said so), macOS 27, no sudo on the Mac
# decides: registry-traffic-read and secrets-held-outside (whether Silo's certificate must be added to every simulator), simulators-for-parallel-agents (a step for each simulator made)
# question: Does an app in a simulator trust a certificate authority that the computer's system trusts, and does it after `simctl keychain add-root-cert`?
# outcome separate-store: The app refused the server before, still refused it once the computer's
#   system trusted the authority, and accepted it after add-root-cert. Every simulator Silo or an
#   agent makes needs Silo's certificate added to it.
# outcome shares-system-store: The app accepted the server once the computer's system trusted the
#   authority. Nothing more is needed for simulators.
# outcome system-trust-refused: Adding the authority to the computer's system store failed over SSH
#   (as in test 21); the simulator step was still tried and its result is in the sentence.
# outcome never-trusted: The app refused the server even after add-root-cert.
# outcome inconclusive: something else went wrong; the sentence and logs say what.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"
. "$KIT_ROOT/lib/ios.sh"

guest_local && kit_skip "needs a Mac: the simulator only runs on macOS"
kit_need_mac 27
kit_need_silovm
ios_need_xcode_vm
kit_confirm "This starts a clone of the Xcode computer for about 20 minutes, makes a test certificate authority in it and trusts it there (never on this Mac)."
trap guest_cleanup EXIT
ios_up trust
build="$(ios_build_app 2>&1)"
echo "$build" | grep -q built || { kit_outcome inconclusive "the test app did not build: $(echo "$build" | tail -1 | cut -c1-200)"; exit 0; }

# A test authority and a server for localhost, in the computer. The simulator shares the
# computer's network, so https://localhost:8443 from the app reaches it.
guest_sh 'set -e; mkdir -p $HOME/silokit/tls; cd $HOME/silokit/tls
cat > ca.cnf <<CNF
[req]
distinguished_name = dn
prompt = no
[dn]
CN = Silo Kit Test CA
[v3_ca]
basicConstraints = critical, CA:TRUE
keyUsage = critical, keyCertSign, cRLSign
CNF
cat > leaf.cnf <<CNF
[req]
distinguished_name = dn
prompt = no
[dn]
CN = localhost
[v3_leaf]
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = DNS:localhost, IP:127.0.0.1
CNF
openssl req -x509 -newkey rsa:2048 -nodes -keyout ca.key -out ca.crt -days 30 -config ca.cnf -extensions v3_ca 2>/dev/null
openssl req -new -newkey rsa:2048 -nodes -keyout leaf.key -out leaf.csr -config leaf.cnf 2>/dev/null
openssl x509 -req -in leaf.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out leaf.crt -days 30 -extfile leaf.cnf -extensions v3_leaf 2>/dev/null
nohup openssl s_server -accept 8443 -cert leaf.crt -key leaf.key -www > server.log 2>&1 < /dev/null &
echo started' >/dev/null 2>&1 || { kit_outcome inconclusive "could not make the test authority or start the server"; exit 0; }

udid="$(ios_new_device kit-trust)"
[ -n "$udid" ] || { kit_outcome inconclusive "could not create a simulator"; exit 0; }
guest_sh "$GUEST_LIMIT limit 300 xcrun simctl bootstatus '$udid' -b" >/dev/null 2>&1
guest_sh "xcrun simctl install '$udid' \$HOME/silokit/KitApp.app" >/dev/null 2>&1

# fetch: launch the app with SILOKIT_URL and print its SILOKIT-FETCH line.
fetch() {
  guest_sh "$GUEST_LIMIT SIMCTL_CHILD_SILOKIT_URL=https://localhost:8443/ limit 60 xcrun simctl launch --console-pty --terminate-running-process '$udid' dev.silo.kitapp 2>&1" |
    tr -d '\r' | grep -m1 'SILOKIT-FETCH' || echo "SILOKIT-FETCH none"
}
before="$(fetch)"; kit_fact before "$before"
sys="$(guest_sudo "security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain /Users/$VM_USER/silokit/tls/ca.crt 2>&1; echo rc=\$?" | tail -1)"
kit_fact system_trust "$sys"
after_system="$(fetch)"; kit_fact after_system_trust "$after_system"
add="$(guest_sh "xcrun simctl keychain '$udid' add-root-cert \$HOME/silokit/tls/ca.crt 2>&1; echo rc=\$?" | tail -1)"
kit_fact add_root_cert "$add"
after_add="$(fetch)"; kit_fact after_add_root_cert "$after_add"
guest_sh "xcrun simctl shutdown '$udid'; xcrun simctl delete '$udid'; pkill -f 's_server -accept 8443'" >/dev/null 2>&1

summary="before: $before; after the system trusts it ($sys): $after_system; after add-root-cert ($add): $after_add"
case "$before" in *" ok "*) kit_outcome inconclusive "the app accepted the test server before anything was trusted: $summary"; exit 0 ;; *none*) kit_outcome inconclusive "the app printed nothing: $summary"; exit 0 ;; esac
if [ "$sys" != rc=0 ]; then kit_outcome system-trust-refused "$summary"
elif echo "$after_system" | grep -q ' ok '; then kit_outcome shares-system-store "$summary"
elif echo "$after_add" | grep -q ' ok '; then kit_outcome separate-store "$summary"
else kit_outcome never-trusted "$summary"
fi
