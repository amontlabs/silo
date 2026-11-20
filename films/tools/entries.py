"""entries.py: reads the entries and statements a film shows from plan/, at build time.

Uses only the standard library, so it runs with any Python 3 and needs no environment.
`shown(ids)` gives, for each id in order, a dict:
  id      the id
  kind    word, principle, property, decision or feature
  text    the text: a word's meaning, a principle's or a property's `says`, a decision's
          `answer` (its `asks` while it has none), a feature's `what`
  asks    a decision's question, when it has an answer too
  status  "in force" (a statement, or an entry the owner took), "replaced" (a decision that a
          later one took the place of), "not decided" (no verdict) or "dropped" (the owner said no)
  sets    for an entry not decided: every statement a yes would set, in order, as
          { "in": the list it goes to, "name": its id or its word, "text": its `says` or `means` }
  removes for an entry not decided: the ids a yes would take out of the lists
A yes covers exactly what the entry shows, so the page prints `sets` and `removes` with it.
An id that is in no list and no entry file of plan/ stops the build.
"""
import json, pathlib

PLAN = pathlib.Path(__file__).resolve().parent.parent.parent / "plan"


def word_id(word):
    return word.lower().replace(" ", "-")


def read_plan(plan=PLAN):
    """Every statement and every entry of plan/, by id."""
    found = {}
    if not plan.is_dir():
        raise SystemExit(f"{plan} is missing: the films are built inside the Silo repository.")
    for path in sorted(plan.rglob("*.json")):
        rel = path.relative_to(plan)
        if any(p.startswith(".") for p in rel.parts) or rel.name in ("order.json", "parts.json"):
            continue
        data = json.loads(path.read_text())
        if isinstance(data, list):
            for s in data:
                if "word" in s:
                    found[word_id(s["word"])] = {"id": word_id(s["word"]), "kind": "word",
                                                 "text": f'{s["word"]}: {s["means"]}', "status": "in force"}
                else:
                    kind = "principle" if rel.name == "principles.json" else "property"
                    found[s["id"]] = {"id": s["id"], "kind": kind, "text": s["says"], "status": "in force"}
        elif rel.parent.name in ("decisions", "features"):
            eid = path.stem
            status = {"take": "in force", "drop": "dropped"}.get(data.get("verdict"), "not decided")
            if data.get("replaced_by"):
                status = "replaced"
            if rel.parent.name == "decisions":
                e = {"id": eid, "kind": "decision", "text": data.get("answer") or data["asks"], "status": status}
                if data.get("answer"):
                    e["asks"] = data["asks"]
            else:
                e = {"id": eid, "kind": "feature", "text": f'{data["name"]}: {data["what"]}', "status": status}
            if status == "not decided":
                e["sets"] = [{"in": s["in"], "name": s.get("id") or s["word"], "text": s.get("says") or s["means"]}
                             for s in data.get("sets", [])]
                e["removes"] = list(data.get("removes", []))
            found[eid] = e
    return found


def shown(ids, plan=PLAN):
    found = read_plan(plan)
    missing = [i for i in ids if i not in found]
    if missing:
        raise SystemExit("film.json shows ids that plan/ does not hold: " + ", ".join(missing))
    return [found[i] for i in ids]


if __name__ == "__main__":
    import sys
    for e in shown(sys.argv[1:]):
        print(f'{e["id"]} ({e["kind"]}, {e["status"]}): {e["text"]}')
        for s in e.get("sets", []):
            print(f'    sets {s["in"]} {s["name"]}: {s["text"]}')
        for r in e.get("removes", []):
            print(f'    removes {r}')
