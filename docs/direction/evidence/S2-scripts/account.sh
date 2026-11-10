#!/bin/zsh
# usage: account.sh <sandbox>   (run from the repository root)
# Runs the engine's working-account setup in a running computer, as `working_account::prepare` does.
. ~/.s2/env.sh
G=app/SiloUI/src-tauri/guest
$MSB_PATH exec "$1" --no-start --no-tty --quiet --timeout 30m --user root --workdir / -- /bin/sh -c "$(cat $G/working-account.sh)" sh "$(cat $G/working-account.py)" "$(cat $G/desktop-service.py)"
