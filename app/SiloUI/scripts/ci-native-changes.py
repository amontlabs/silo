"""Decide whether a pull request needs the native (Rust) CI jobs.

Reads changed paths (one per line, relative to the repository root) on stdin
and prints native=true or native=false for $GITHUB_OUTPUT. Only paths that no
Rust crate, build script or Rust test reads are skippable; anything else, and
an empty or unreadable list, keeps the native jobs running.
"""
import sys

# Top-level trees that only the website, demo films and documentation use.
SKIPPABLE_PREFIXES = ('website/', 'demo/', 'docs/', 'artifacts/', 'assets/')


def skippable(path):
    if path.startswith(SKIPPABLE_PREFIXES):
        return True
    # Top-level Markdown only: Markdown inside app/ can be read by tests.
    return '/' not in path and path.endswith('.md')


def needs_native(paths):
    paths = [path.strip() for path in paths if path.strip()]
    return not paths or not all(skippable(path) for path in paths)


if __name__ == '__main__':
    print(f"native={'true' if needs_native(sys.stdin) else 'false'}")
