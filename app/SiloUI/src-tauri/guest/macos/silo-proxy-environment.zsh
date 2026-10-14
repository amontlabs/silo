#!/bin/zsh
set -eu

SUPPORT='/Library/Application Support/Silo/NetworkBootstrap'
PLACEHOLDERS="$SUPPORT/placeholders.zsh"
NAMES="$SUPPORT/placeholder-names.txt"
RETIRED="$SUPPORT/retired-names.txt"

[[ -r "$PLACEHOLDERS" ]] || exit 0
source "$PLACEHOLDERS"
[[ -r "$NAMES" ]] || exit 0

while IFS= read -r name; do
  [[ "$name" =~ '^[A-Za-z_][A-Za-z0-9_]*$' ]] || continue
  value="${(P)name}"
  /bin/launchctl setenv "$name" "$value"
done < "$NAMES"

if [[ -r "$RETIRED" ]]; then
  while IFS= read -r name; do
    [[ "$name" =~ '^[A-Za-z_][A-Za-z0-9_]*$' ]] || continue
    /bin/launchctl unsetenv "$name"
  done < "$RETIRED"
fi
