"""behind.py <film>: says whether a film is behind the plan. Standard library only, runs with any Python 3.

A film is behind when an entry it shows has changed since it was built. For each id under `shows` in
the film's film.json, it finds the file of plan/ that holds the id, and lists the commits that touched
that file later than the last commit that touched the film's folder. "Later" compares commit times, and
merges are left out: the plan and the films are often written on branches side by side, and a merge
would make every file it brings in look new. It prints, per id, the file and those
commits, or that the film is up to date. Changes to plan/ not yet committed are named too.
Exit status: 0 when up to date, 1 when behind.
"""
import json, pathlib, subprocess, sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import place  # noqa: E402
from entries import PLAN, word_id  # noqa: E402

ROOT = PLAN.parent


def git(*args):
    return subprocess.run(["git", "-C", str(ROOT), *args], check=True, capture_output=True, text=True).stdout


def holders():
    """The file of plan/ that holds each id, by id, found the way entries.py finds them."""
    found = {}
    for path in sorted(PLAN.rglob("*.json")):
        rel = path.relative_to(PLAN)
        if any(p.startswith(".") for p in rel.parts) or rel.name in ("order.json", "parts.json"):
            continue
        data = json.loads(path.read_text())
        if isinstance(data, list):
            for s in data:
                found[word_id(s["word"]) if "word" in s else s["id"]] = path
        elif rel.parent.name in ("decisions", "features"):
            found[path.stem] = path
    return found


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: behind.py <film>")
    film = place.film(sys.argv[1])
    shows = json.loads((film / "film.json").read_text()).get("shows", [])
    last = git("log", "-1", "--no-merges", "--format=%ct %h", "--", str(film)).split()
    if not last:
        sys.exit(f"{film.name}: no commit touches this film's folder yet.")
    built, built_id = int(last[0]), last[1]
    found = holders()
    behind = False
    print(f"{film.name}: last changed in {built_id}")
    for eid in shows:
        path = found.get(eid)
        if path is None:
            print(f"  {eid}: plan/ holds no such id")
            behind = True
            continue
        rel = path.relative_to(ROOT)
        later = [line.split(" ", 1)[1]
                 for line in git("log", "--no-merges", "--format=%ct %h %s", "--", str(rel)).splitlines()
                 if int(line.split(" ", 1)[0]) > built]
        dirty = git("status", "--porcelain", "--", str(rel)).strip()
        if later or dirty:
            behind = True
            print(f"  {eid}: {rel} changed since")
            for line in later:
                print(f"      {line}")
            if dirty:
                print("      (changes not committed yet)")
    if not behind:
        print(f"  up to date: none of the {len(shows)} entries it shows changed since")
    sys.exit(1 if behind else 0)


if __name__ == "__main__":
    main()
