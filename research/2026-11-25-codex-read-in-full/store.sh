#!/bin/sh
# Runs a command with the test authority in the system's own store ($STORE bound over the
# store's file), in a mount namespace of its own, so only that command sees it.
exec unshare -m sh -c 'mount --bind "$STORE" /etc/ssl/certs/ca-certificates.crt && exec "$@"' - "$@"
