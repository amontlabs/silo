#!/usr/bin/env python3
"""Run disposable production-runtime tests on Linux; never pretend no KVM passed."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import urllib.request

root = Path(__file__).resolve().parent.parent
evidence = root / "test-results/linux"
evidence.mkdir(parents=True, exist_ok=True)
report = {"architecture": platform.machine(), "kvm": False, "tests": []}
try:
    with open("/dev/kvm", "rb+", buffering=0) as device:
        assert fcntl.ioctl(device, 0xAE00, 0) == 12, "Unsupported KVM API"
        vm = fcntl.ioctl(device, 0xAE01, 0)
        os.close(vm)
    report["kvm"] = True
except (OSError, AssertionError) as error:
    report["reason"] = str(error)
    (evidence / "runtime.json").write_text(json.dumps(report, indent=2))
    print("Hardware VM tests BLOCKED: " + str(error))
    # Missing hardware is a failed verification, not a successful skipped test.
    sys.exit(2)

target = subprocess.check_output(["rustc", "--print", "host-tuple"], text=True).strip()
environment = dict(os.environ)
environment["SILO_TEST_MSB"] = str(root / f"src-tauri/binaries/msb-{target}")
environment["SILO_TEST_LIBKRUNFW"] = str(root / f"src-tauri/runtime/microsandbox/{target}/lib/libkrunfw.so.5.6.1")
environment["SILO_LIVE_TEST_CONFIRM"] = "disposable-test-fixtures"
# A short path on the runner's disk: /tmp may be a small tmpfs.
environment["SILO_TEST_TMP"] = "/var/tmp/silo-t"
Path(environment["SILO_TEST_TMP"]).mkdir(parents=True, exist_ok=True)

# The tests import the pinned public guest image the app downloads on first use.
lock = json.loads((root / "guest-image/image-lock.json").read_text())
key = {"x86_64": "amd64", "aarch64": "arm64"}[platform.machine()]
image = lock["images"][key]
download = root / f"src-tauri/target/guest-image-download/{image['version']}/image-{key}.tar.gz"


def verified(path):
    if not path.is_file() or path.stat().st_size != image["archiveBytes"]:
        return False
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1 << 20):
            digest.update(chunk)
    return digest.hexdigest() == image["archiveSha256"]


if not verified(download):
    download.parent.mkdir(parents=True, exist_ok=True)
    partial = download.with_suffix(".partial")
    with urllib.request.urlopen(f"{lock['releaseUrl']}/image-{key}.tar.gz", timeout=60) as response, \
            partial.open("wb") as output:
        shutil.copyfileobj(response, output, 1 << 20)
    partial.replace(download)
    if not verified(download):
        download.unlink()
        sys.exit(f"Guest image image-{key}.tar.gz does not match guest-image/image-lock.json")
environment["SILO_TEST_GUEST_ARCHIVE"] = str(download)
published = root / f"src-tauri/runtime/guest-image/{image['version']}/image.tar.gz"
if not verified(published):
    published.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(download, published)

for test in [
    "live_downloaded_image_import_and_cache_reuse",
    "github_guest_bootstrap_and_live_identity",
    "real_backup_restore_preserves_root_and_computer_without_original_cache",
    "live_secret_adapter_uses_refs_and_preserves_boot_for_live_updates",
    "lifecycle_recovery_survives_real_worker_exit_without_repeating_restart",
]:
    returncode = 124
    with (evidence / f"{test}.log").open("w") as output:
        try:
            result = subprocess.run(
                ["cargo", "test", "--manifest-path", "src-tauri/Cargo.toml", "--locked", test,
                 "--", "--ignored", "--test-threads=1", "--nocapture"],
                cwd=root, env=environment, stdout=output, stderr=subprocess.STDOUT,
                timeout=600,
            )
            returncode = result.returncode
        except subprocess.TimeoutExpired:
            output.write("\nHardware test exceeded its ten-minute limit.\n")
    output_text = (evidence / f"{test}.log").read_text()
    passed = returncode == 0 and "test result: ok. 1 passed; 0 failed;" in output_text
    report["tests"].append({"name": test, "passed": passed})
    (evidence / "runtime.json").write_text(json.dumps(report, indent=2))
    if not passed:
        print(f"FAILED: {test}; see {evidence}")
        sys.exit(returncode or 1)
    print("PASS: " + test)
