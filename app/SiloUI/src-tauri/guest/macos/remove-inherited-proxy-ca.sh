#!/bin/sh
set -eu

fingerprint=${1:?missing CA fingerprint}
keychain=${2:?missing keychain path}
case "$fingerprint" in
	*[!0123456789abcdefABCDEF]*|'') exit 2 ;;
esac
[ "${#fingerprint}" -eq 64 ] || exit 2

certificates=$(/usr/bin/security find-certificate -a -Z "$keychain")
if printf '%s\n' "$certificates" | /usr/bin/grep -Fqi "$fingerprint"; then
	# Removing the trust settings needs an authorization that an SSH session cannot give
	# ("no user interaction was possible"). The certificate itself can still go; a trust
	# setting left for a certificate that is no longer in the keychain trusts nothing.
	/usr/bin/security delete-certificate -t -Z "$fingerprint" "$keychain" ||
		/usr/bin/security delete-certificate -Z "$fingerprint" "$keychain"
fi

remaining=$(/usr/bin/security find-certificate -a -Z "$keychain")
if printf '%s\n' "$remaining" | /usr/bin/grep -Fqi "$fingerprint"; then
	printf '%s\n' 'The inherited Silo proxy CA remains in the target keychain.' >&2
	exit 1
fi
