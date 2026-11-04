"""Decide which native (Rust) CI work a pull request needs.

Reads changed paths (one per line, relative to the repository root) on stdin
and prints two lines for $GITHUB_OUTPUT:

  native=true|false  whether any Rust job must run at all
  engine=true|false  whether the silo-engine suite (Linux job and the engine
                     tests in the macOS job) must run

A path counts as unaffecting only when no Rust crate, build script or Rust test
in that scope reads it. Anything else, and an empty list, runs everything.
"""
import sys

# Top-level trees that only the website, demo films and documentation use.
SKIPPABLE_PREFIXES = ('website/', 'demo/', 'docs/', 'artifacts/', 'assets/')

APP = 'app/SiloUI/'
# Frontend sources. Rust reads only these two locations inside them.
FRONTEND_PREFIX = APP + 'src/'
FRONTEND_READ_BY_RUST = (APP + 'src/test/contracts/', APP + 'src/features/application/model/reserved-secret-names.json')
# Crates that silo-engine does not depend on (it depends on silo-api and silo-host only).
NOT_ENGINE_PREFIXES = (APP + 'src-tauri/src/', APP + 'src-tauri/silod/', APP + 'client/')
# Files in those crates that silo-engine tests read anyway.
READ_BY_ENGINE = (APP + 'src-tauri/src/commands/remote.rs',)


def skippable(path):
    if path.startswith(SKIPPABLE_PREFIXES):
        return True
    # Top-level Markdown only: Markdown inside app/ can be read by tests.
    return '/' not in path and path.endswith('.md')


def frontend_only(path):
    return path.startswith(FRONTEND_PREFIX) and not path.startswith(FRONTEND_READ_BY_RUST)


def native_unaffected(path):
    return skippable(path) or frontend_only(path)


def engine_unaffected(path):
    if path.startswith(READ_BY_ENGINE):
        return False
    return native_unaffected(path) or path.startswith(NOT_ENGINE_PREFIXES)


def clean(paths):
    return [path.strip() for path in paths if path.strip()]


def needs_native(paths):
    paths = clean(paths)
    return not paths or not all(native_unaffected(path) for path in paths)


def needs_engine(paths):
    paths = clean(paths)
    return not paths or not all(engine_unaffected(path) for path in paths)


if __name__ == '__main__':
    paths = list(sys.stdin)
    print(f"native={'true' if needs_native(paths) else 'false'}")
    print(f"engine={'true' if needs_engine(paths) else 'false'}")
