"""The app's Rust toolchain file must pin the runtime's reviewed toolchain."""
import json
from pathlib import Path
import tomllib
import unittest

APP = Path(__file__).resolve().parent.parent
ROOT = APP.parent.parent


class RustToolchainTests(unittest.TestCase):
    def test_toolchain_file_matches_runtime_inputs(self):
        toolchain = tomllib.loads((ROOT / 'rust-toolchain.toml').read_text())['toolchain']
        runtime = json.loads((APP / 'runtime-inputs.json').read_text())['toolchain']
        self.assertEqual(toolchain['channel'], runtime)


if __name__ == '__main__':
    unittest.main()
