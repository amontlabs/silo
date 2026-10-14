#!/bin/sh
# Git and Git LFS receive the stable placeholder, never a host token.
set -eu
[ "${1:-}" = get ] || exit 0
protocol=
host=
while IFS= read -r line && [ -n "$line" ]; do
    case "$line" in
        protocol=*) protocol=${line#protocol=} ;;
        host=*) host=${line#host=} ;;
    esac
done
[ "$protocol" = https ] && [ "$host" = github.com ] || exit 0
printf '%s\n' 'username=x-access-token' 'password=$MSB_SILO_GITHUB'
