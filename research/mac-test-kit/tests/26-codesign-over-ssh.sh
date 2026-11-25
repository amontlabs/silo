#!/bin/bash
# id: 26
# title: codesign in a computer, over SSH, with no dialog
# tier: vm
# time: about 15 minutes
# needs: the Xcode computer of test 23, macOS 27 on the Mac; no Apple account (a throwaway self-signed identity is made in the computer)
# decides: apple-keys-in-a-computer (whether a development identity in the computer works for agents with nobody at the screen)
# question: Over SSH, does codesign sign with an identity in the login keychain, and with one in a keychain of its own set up the way CI does it, without a dialog on the screen?
# outcome both-sign: Both keychains signed over SSH. Nothing special is needed for a development
#   identity in a computer.
# outcome own-keychain-only: The login keychain failed or waited, and a keychain of its own,
#   unlocked in the same session with its partition list set, signed. Silo's Apple development
#   piece sets that keychain up; the fact login_after_unlock says whether unlocking the login
#   keychain with the user's password would have been enough.
# outcome neither: Neither signed; the errors are in the sentence. errSecInternalComponent means
#   the keychain was locked; a time-out means a dialog waited on the screen.
# outcome inconclusive: Something else went wrong; the sentence and the attached logs say what.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"
. "$KIT_ROOT/lib/ios.sh"

guest_local && kit_skip "needs a Mac: codesign and keychains only exist on macOS"
kit_need_mac 27
kit_need_silovm
ios_need_xcode_vm
kit_confirm "This starts a clone of the Xcode computer for about 15 minutes and makes a throwaway signing identity in it."
trap guest_cleanup EXIT
ios_up sign

# A self-signed code-signing identity and a small binary to sign, made in the computer.
make="$(guest_sh 'set -e; mkdir -p $HOME/silokit/sign; cd $HOME/silokit/sign
cat > id.cnf <<CNF
[req]
distinguished_name = dn
prompt = no
[dn]
CN = Silo Kit Signing
[ext]
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
CNF
openssl req -x509 -newkey rsa:2048 -nodes -keyout id.key -out id.crt -days 2 -config id.cnf -extensions ext 2>/dev/null
openssl pkcs12 -export -inkey id.key -in id.crt -out id.p12 -passout pass:silokit 2>/dev/null
printf "int main(void){return 0;}\n" > t.c && cc -o t1 t.c && cp t1 t2 && cp t1 t3
openssl x509 -in id.crt -noout -fingerprint -sha1 | sed "s/.*=//; s/://g"' 2>&1)"
sha="$(echo "$make" | tail -1)"
case "$sha" in [0-9A-F][0-9A-F][0-9A-F][0-9A-F]*) ;; *) kit_outcome inconclusive "could not make the identity: $(echo "$make" | tail -2 | tr '\n' ' ' | cut -c1-200)"; exit 0 ;; esac

log="$KIT_WORK/codesign.log"
guest_sh "KIT_PW='$(vm_password)'; $GUEST_LIMIT"' cd $HOME/silokit/sign
echo "# login keychain, as the SSH session finds it"
security import id.p12 -k "$HOME/Library/Keychains/login.keychain-db" -P silokit -T /usr/bin/codesign 2>&1
limit 90 codesign -f -s '"$sha"' t1 2>&1; echo "login=$?"
echo "# login keychain, unlocked with the user'"'"'s password in this session"
security unlock-keychain -p "$KIT_PW" "$HOME/Library/Keychains/login.keychain-db" 2>&1
limit 90 codesign -f -s '"$sha"' t3 2>&1; echo "unlocked=$?"
echo "# keychain of its own"
security create-keychain -p kitpw kit.keychain 2>&1
security set-keychain-settings kit.keychain 2>&1
security unlock-keychain -p kitpw kit.keychain 2>&1
security import id.p12 -k kit.keychain -P silokit -T /usr/bin/codesign 2>&1
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k kitpw kit.keychain >/dev/null 2>&1
security list-keychains -d user -s kit.keychain $(security list-keychains -d user | tr -d "\"") 2>&1
limit 90 codesign -f -s '"$sha"' --keychain "$HOME/Library/Keychains/kit.keychain-db" t2 2>&1; echo "own=$?"
codesign -dv t2 2>&1 | grep -i authority' > "$log" 2>&1
kit_attach "$log"
guest_sh 'security delete-keychain kit.keychain' >/dev/null 2>&1

login="$(sed -n 's/^login=//p' "$log")"; own="$(sed -n 's/^own=//p' "$log")"; unlocked="$(sed -n 's/^unlocked=//p' "$log")"
kit_fact login_keychain_status "${login:-none}"
kit_fact login_after_unlock "${unlocked:-none}"
kit_fact own_keychain_status "${own:-none}"
err() { grep -m1 -iE 'errSec|error|fail' "$log" | cut -c1-160; }
case "$login:$own" in
  0:0) kit_outcome both-sign "codesign signed over SSH from the login keychain and from a keychain of its own" ;;
  *:0) kit_outcome own-keychain-only "the login keychain ended with $login (124 is the time limit), $unlocked once unlocked with the user's password; a keychain of its own signed. $(err)" ;;
  *) kit_outcome neither "login keychain: $login, unlocked: $unlocked, own keychain: $own (124 is the time limit). $(err)" ;;
esac
