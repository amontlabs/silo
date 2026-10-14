"""Assemble a complete versioned draft, or publish a verified existing draft."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
from datetime import datetime, timezone

PLATFORMS = {"darwin-aarch64": "Silo-macos-arm64.app.tar.gz"}
PACKAGES = set(PLATFORMS.values()) | {"Silo-macos-arm64.dmg"}
EXPECTED = PACKAGES | {name + ".sig" for name in PLATFORMS.values()}


def gh(*args):
    return subprocess.check_output(["gh", *args], text=True).strip()


def validate_version(version):
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version) or version == "0.0.0":
        raise RuntimeError("Use a nonzero stable version such as 0.1.0.")
    return tuple(map(int, version.split(".")))


DOWNLOADS = (
    ("macOS (Apple Silicon)", (("DMG", "Silo-macos-arm64.dmg"),)),
)
CHANGE_LISTS = re.compile(r"^### (Major|Minor|Patch) Changes$", re.M)


def release_page(version, repository, notes):
    """GitHub release body: downloads first, then the notes with the full change lists collapsed.

    The update feed keeps the plain notes; Silo shows them as text.
    """
    base = f"https://github.com/{repository}/releases/download/v{version}"
    rows = [" · ".join(f"[{label}]({base}/{name})" for label, name in files) for _, files in DOWNLOADS]
    downloads = "\n".join(["| Download | |", "| --- | --- |", *(f"| {platform} | {row} |" for (platform, _), row in zip(DOWNLOADS, rows))])
    body = re.sub(r"\A# Silo [^\n]*\n+", "", notes.strip())
    lists = CHANGE_LISTS.search(body)
    if lists:
        summary, changes = body[:lists.start()].rstrip(), body[lists.start():].rstrip()
        count = len(re.findall(r"^- ", changes, re.M))
        collapsed = f"<details>\n<summary>All changes ({count})</summary>\n\n{changes}\n\n</details>"
        body = f"{summary}\n\n{collapsed}" if summary else collapsed
    return f"{downloads}\n\n{body}\n"


def prepare(root, version, repository, notes):
    validate_version(version)
    actual = {p.name for p in root.iterdir()}
    if actual != EXPECTED or any(not (root / n).is_file() or (root / n).is_symlink() or (root / n).stat().st_size == 0 for n in EXPECTED):
        raise RuntimeError("Release assets are missing, empty, or unexpected; keeping previous releases unchanged.")
    platforms = {}
    for target, name in PLATFORMS.items():
        signature = (root / (name + ".sig")).read_text().strip()
        # Cryptographic verification is done in each build job with the release public key.
        if not signature or len(signature) > 4096 or any(c.isspace() for c in signature):
            raise RuntimeError("Invalid updater signature encoding.")
        platforms[target] = {"url": f"https://github.com/{repository}/releases/download/v{version}/{name}", "signature": signature}
    (root / "latest.json").write_text(json.dumps({"version": version, "notes": notes, "pub_date": datetime.now(timezone.utc).isoformat(), "platforms": platforms}, indent=2) + "\n")
    checksums = []
    for name in sorted(EXPECTED | {"latest.json"}):
        with (root / name).open("rb") as source:
            checksums.append(f"{hashlib.file_digest(source, 'sha256').hexdigest()}  {name}\n")
    (root / "SHA256SUMS").write_text("".join(checksums))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("version")
    parser.add_argument("--publish", action="store_true")
    parser.add_argument("--allow-stable", action="store_true")
    args = parser.parse_args()
    version = args.version
    current = validate_version(version)
    if current[0] >= 1 and not args.allow_stable:
        raise RuntimeError("Silo releases stay below 1.0.0; use --allow-stable only for an approved stable release.")
    repository = os.environ["GH_REPO"]
    tag = f"v{version}"
    releases = json.loads(gh("api", "--paginate", "--slurp", f"repos/{repository}/releases"))
    releases = [release for page in releases for release in page]
    existing = next((r for r in releases if r["tag_name"] == tag), None)
    published = [r for r in releases if not r["draft"] and not r["prerelease"] and re.fullmatch(r"v\d+\.\d+\.\d+", r["tag_name"])]
    if any(validate_version(r["tag_name"][1:]) >= current for r in published):
        raise RuntimeError("Version must be newer than every published stable release.")
    if args.publish:
        if not existing or not existing["draft"]:
            raise RuntimeError("A verified draft is required; existing public releases are never changed.")
        assets = {a["name"] for a in existing["assets"]}
        if assets != EXPECTED | {"latest.json", "SHA256SUMS"} or any(a["size"] <= 0 for a in existing["assets"]):
            raise RuntimeError("Draft is incomplete; refusing to publish.")
        # Download and verify every byte again after draft storage and before public cutover.
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            gh("release", "download", tag, "--dir", directory)
            root = Path(directory)
            lines = (root / "SHA256SUMS").read_text().splitlines()
            if len(lines) != len(EXPECTED) + 1 or {line.split("  ", 1)[-1] for line in lines} != EXPECTED | {"latest.json"}:
                raise RuntimeError("Draft checksums are incomplete or duplicated.")
            for line in lines:
                digest, name = line.split("  ", 1)
                if name not in EXPECTED | {"latest.json"}:
                    raise RuntimeError("Unexpected checksum entry.")
                with (root / name).open("rb") as source:
                    if hashlib.file_digest(source, "sha256").hexdigest() != digest:
                        raise RuntimeError("Draft checksum mismatch.")
            feed = json.loads((root / "latest.json").read_text())
            if feed["version"] != version or set(feed["platforms"]) != set(PLATFORMS):
                raise RuntimeError("Draft update feed does not match this version.")
            if feed.get('notes') is not None and not isinstance(feed['notes'], str):
                raise RuntimeError("Draft update feed has invalid notes.")
            date = feed.get('pub_date')
            if date is not None:
                if not isinstance(date, str) or not re.fullmatch(
                        r'[0-9]{4}-[0-9]{2}-[0-9]{2}[Tt][0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?(?:[Zz]|[+-][0-9]{2}:[0-5][0-9])', date):
                    raise RuntimeError("Draft update feed has an invalid publication date.")
                try:
                    datetime.fromisoformat(date.removesuffix('Z').removesuffix('z') + ('+00:00' if date[-1] in 'Zz' else ''))
                except ValueError:
                    raise RuntimeError("Draft update feed has an invalid publication date.") from None
            for target, name in PLATFORMS.items():
                expected_url = f"https://github.com/{repository}/releases/download/{tag}/{name}"
                if feed['platforms'][target] != {'url': expected_url, 'signature': (root / (name + '.sig')).read_text().strip()}:
                    raise RuntimeError("Draft update feed has an unexpected URL or signature.")
            subprocess.run(["python3", str(Path(__file__).with_name("verify-release-signatures.py")), str(root)], check=True)
            subprocess.run(["python3", str(Path(__file__).with_name("verify-release-metadata.py")), str(root), version], check=True)
        gh("release", "edit", tag, "--draft=false", "--latest", "--prerelease=false")
    else:
        if existing:
            raise RuntimeError("Release already exists; never overwrite a draft or public release.")
        sha = os.environ["GITHUB_SHA"]
        resolved = gh("api", f"repos/{repository}/commits/{tag}", "--jq", ".sha")
        if resolved != sha:
            raise RuntimeError("Version tag does not identify this build.")
        root = Path("release-assets")
        notes = Path("release-notes.md")
        if not notes.is_file() or not notes.read_text().strip():
            raise RuntimeError("Release notes are required.")
        prepare(root, version, repository, notes.read_text())
        page = Path("release-page.md")
        page.write_text(release_page(version, repository, notes.read_text()))
        gh("release", "create", tag, *[str(root / name) for name in sorted(EXPECTED | {"latest.json", "SHA256SUMS"})], "--verify-tag", "--draft", "--title", f"Silo {version}", "--notes-file", str(page))


if __name__ == "__main__":
    main()
