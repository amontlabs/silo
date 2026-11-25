"""The publishing side: keys, root files, the index and the price tables, as static files.

usage: publish.py init <keys dir> <site dir>
       publish.py release <keys dir> <site dir> <prices.json> <app version> [--root] [--days N]

`init` makes a root key and an online key and writes root-1.json, signed by the root key. In Silo
the root key lives on a hardware key the owner holds, used for a root file and for a cut beyond the
floor; the online key lives with the daily pipeline. `release` writes prices-<version>.json and a new
index.json naming it, both signed by the online key, the table also by the root key with --root."""
import datetime, hashlib, json, sys
from pathlib import Path
import sig


def now(): return datetime.datetime.now(datetime.UTC).replace(microsecond=0)


def make_root(keys_dir, version, root_ids, online, threshold=1, expires_days=120):
    """`online`: list of (id, public). Returns the root file's bytes."""
    keys = [{"id": i, "role": "root", "public": sig.public_of(Path(keys_dir) / f"{i}.key")} for i in root_ids]
    for i, pub, *rest in online:
        keys.append({"id": i, "role": "online", "public": pub,
                     "expires": rest[0] if rest else (now() + datetime.timedelta(days=expires_days)).isoformat()})
    return json.dumps({"format": 1, "version": version, "threshold": threshold, "keys": keys}, indent=1).encode()


def write_signed(site, name, data, signers):
    """signers: list of (key id, key path)."""
    (Path(site) / name).write_bytes(data)
    (Path(site) / (name + ".sig")).write_text("".join(sig.sign(data, k, p) for k, p in signers))


def make_index(version, prices_bytes, app_version, at, days=2):
    t = json.loads(prices_bytes)
    return json.dumps({
        "format": 1, "version": version, "published": at.isoformat(),
        "expires": (at + datetime.timedelta(days=days)).isoformat(),
        "app": {"version": app_version},
        "prices": {"version": t["version"], "path": f"prices-{t['version']}.json",
                   "sha256": hashlib.sha256(prices_bytes).hexdigest()},
    }, indent=1).encode()


def main():
    cmd, keys, site = sys.argv[1:4]
    Path(keys).mkdir(parents=True, exist_ok=True); Path(site).mkdir(parents=True, exist_ok=True)
    if cmd == "init":
        sig.keygen(Path(keys) / "root-a.key")
        pub = sig.keygen(Path(keys) / "online-1.key")
        write_signed(site, "root-1.json", make_root(keys, 1, ["root-a"], [("online-1", pub)]), [("root-a", Path(keys) / "root-a.key")])
    elif cmd == "release":
        prices, app = sys.argv[4], sys.argv[5]
        days = int(sys.argv[sys.argv.index("--days") + 1]) if "--days" in sys.argv else 2
        data = Path(prices).read_bytes()
        v = json.loads(data)["version"]
        signers = [("online-1", Path(keys) / "online-1.key")] + ([("root-a", Path(keys) / "root-a.key")] if "--root" in sys.argv else [])
        write_signed(site, f"prices-{v}.json", data, signers)
        old = Path(site) / "index.json"
        iv = json.loads(old.read_text())["version"] + 1 if old.exists() else 1
        write_signed(site, "index.json", make_index(iv, data, app, now(), days), [("online-1", Path(keys) / "online-1.key")])


if __name__ == "__main__":
    main()
