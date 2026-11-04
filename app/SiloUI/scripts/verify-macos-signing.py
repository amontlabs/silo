"""Verify release signatures and the exact engine policy without editing the app.

A `developer-id` release (SILO_RELEASE_SIGNING) reconstructs the helper signatures with its
Developer ID identity, so the expected code hashes include its team."""
import os
from pathlib import Path
import sys
from macos_notarization import release_identity
from macos_release_signing import verify_bundle

if __name__ == '__main__':
    verify_bundle(Path(sys.argv[1]).resolve(), release_identity(os.environ))
