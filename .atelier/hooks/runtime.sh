#!/bin/sh
# Prepares the bundled runtime (runtime:prepare). Downloads and compiled builds come from one
# cache shared by every checkout of this user: SILO_RUNTIME_CACHE_DIR when set, otherwise
# ~/Library/Caches/silo-runtime on macOS and ~/.cache/silo-runtime elsewhere.
set -eu
cd "$(dirname "$0")/../.."
if [ -z "${SILO_RUNTIME_CACHE_DIR:-}" ]; then
  case "$(uname -s)" in
    Darwin) SILO_RUNTIME_CACHE_DIR=$HOME/Library/Caches/silo-runtime ;;
    *) SILO_RUNTIME_CACHE_DIR=${XDG_CACHE_HOME:-$HOME/.cache}/silo-runtime ;;
  esac
fi
export SILO_RUNTIME_CACHE_DIR
mkdir -p "$SILO_RUNTIME_CACHE_DIR"
exec npm --prefix app/SiloUI run runtime:prepare
