#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM

fingerprint=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
keychain="$tmp_dir/System.keychain.fixture"
state="$tmp_dir/certificates.txt"
log="$tmp_dir/security.log"
printf 'SHA-256 hash: 0x%s\n' "$fingerprint" >"$state"
: >"$log"

cat >"$tmp_dir/security" <<'SECURITY'
#!/bin/sh
set -eu
operation=$1
shift
case "$operation" in
	find-certificate)
		[ "$#" -eq 3 ] && [ "$1" = -a ] && [ "$2" = -Z ] && [ "$3" = "$TEST_KEYCHAIN" ]
		cat "$TEST_STATE"
		;;
	delete-certificate)
		if [ "$1" = -t ]; then
			# Trust settings cannot be removed without a user session.
			[ -z "${TEST_NO_TRUST_REMOVAL:-}" ] || exit 153
			shift
			printf 'with-trust ' >>"$TEST_LOG"
		fi
		[ "$#" -eq 3 ] && [ "$1" = -Z ]
		[ "$2" = "$TEST_FINGERPRINT" ] && [ "$3" = "$TEST_KEYCHAIN" ]
		set -- "$@"
		printf 'delete %s\n' "$*" >>"$TEST_LOG"
		: >"$TEST_STATE"
		;;
	*) exit 2 ;;
esac
SECURITY
chmod +x "$tmp_dir/security"

# Substitute only the executable path so the production cleanup script runs
# against a synthetic keychain fixture without touching a user Keychain.
sed "s|/usr/bin/security|$tmp_dir/security|g" \
	"$script_dir/remove-inherited-proxy-ca.sh" >"$tmp_dir/remove-inherited-proxy-ca.sh"
chmod +x "$tmp_dir/remove-inherited-proxy-ca.sh"

TEST_KEYCHAIN=$keychain TEST_STATE=$state TEST_LOG=$log TEST_FINGERPRINT=$fingerprint \
	sh "$tmp_dir/remove-inherited-proxy-ca.sh" "$fingerprint" "$keychain"
if grep -Fqi "$fingerprint" "$state"; then
	printf '%s\n' 'the inherited CA fingerprint remains after cleanup' >&2
	exit 1
fi
grep -Fqx "with-trust delete -Z $fingerprint $keychain" "$log"

# Without a user session the trust settings stay, and the certificate still goes.
printf 'SHA-256 hash: 0x%s\n' "$fingerprint" >"$state"
: >"$log"
TEST_NO_TRUST_REMOVAL=1 TEST_KEYCHAIN=$keychain TEST_STATE=$state TEST_LOG=$log TEST_FINGERPRINT=$fingerprint \
	sh "$tmp_dir/remove-inherited-proxy-ca.sh" "$fingerprint" "$keychain"
if grep -Fqi "$fingerprint" "$state"; then
	printf '%s\n' 'the inherited CA remains when trust settings cannot be removed' >&2
	exit 1
fi
grep -Fqx "delete -Z $fingerprint $keychain" "$log"
printf 'SHA-256 hash: 0x%s\n' "$fingerprint" >"$state"
: >"$log"
TEST_KEYCHAIN=$keychain TEST_STATE=$state TEST_LOG=$log TEST_FINGERPRINT=$fingerprint \
	sh "$tmp_dir/remove-inherited-proxy-ca.sh" "$fingerprint" "$keychain"

# A retry after successful deletion is idempotent and does not delete another cert.
TEST_KEYCHAIN=$keychain TEST_STATE=$state TEST_LOG=$log TEST_FINGERPRINT=$fingerprint \
	sh "$tmp_dir/remove-inherited-proxy-ca.sh" "$fingerprint" "$keychain"
[ "$(wc -l <"$log" | tr -d ' ')" -eq 1 ]

if TEST_KEYCHAIN=$keychain TEST_STATE=$state TEST_LOG=$log TEST_FINGERPRINT=$fingerprint \
	sh "$tmp_dir/remove-inherited-proxy-ca.sh" bad "$keychain"; then
	printf '%s\n' 'invalid CA fingerprint unexpectedly passed validation' >&2
	exit 1
fi

printf '%s\n' 'inherited proxy CA cleanup checks passed'
