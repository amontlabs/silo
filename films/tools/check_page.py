"""check_page.py <film dir> < dump: the built page shows every part film.json names, with the right number of
items, and none it does not name. Each entry the film shows is printed with its id; one not decided has its three
buttons and a note, and under its text every statement a yes would set (its list, its id or word, its text) and
every id it would remove; one in force, replaced or dropped has none of these."""
import json, pathlib, re, sys
import entries, place
ch = place.film(sys.argv[1])
C = json.loads((ch / "film.json").read_text())
narr = json.loads((ch / "narration.json").read_text())
dom = sys.stdin.read()
bad = []
kind = C.get("kind")
if kind not in ("teaches", "decides"):
    bad.append(f'film.json: "kind" is {kind!r}; it must be "teaches" or "decides"')
for gone in ("statements", "marks", "promises"):
    if gone in C:
        bad.append(f'film.json has "{gone}": a film states nothing of its own, it names what it shows in "shows"')
sections = {m.group(1): m.group(2) for m in re.finditer(r'<section[^>]*data-part="([a-z]+)"[^>]*>(.*?)</section>', dom, re.S)}
html_text = lambda t: t.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
radios = lambda h: len(re.findall(r'type="radio"', h))
items = lambda h: len(re.findall(r'<li class="st"', h))
want = {"shows": True, "words": True, "copy": True, "clear": bool(C.get("clear")), "test": bool(C.get("test")),
        "wrong": bool(C.get("wrong")),
        "choice": bool(C.get("choice") and C["choice"].get("options"))}
for name, on in want.items():
    if on and name not in sections: bad.append(f'the part "{name}" is in film.json but missing from the page')
    if not on and name in sections: bad.append(f'the page shows the part "{name}", which film.json does not have')
h = sections.get("shows", "")
if h:
    shown = entries.shown(C["shows"])
    lis = re.findall(r'<li class="st" data-id="([^"]*)" data-status="([^"]*)">(.*?)</li>', h, re.S)
    if [i for i, _, _ in lis] != [e["id"] for e in shown]:
        bad.append(f"What the film shows: the page lists {[i for i, _, _ in lis]}, film.json shows {C['shows']}")
    for (i, status, body), e in zip(lis, shown):
        if status != e["status"]: bad.append(f"{i}: the page says {status!r}, plan/ says {e['status']!r}")
        if html_text(e["text"]) not in body: bad.append(f"{i}: its text from plan/ is not on the page")
        if e["status"] == "not decided" and (radios(body) != 3 or "<textarea" not in body):
            bad.append(f"{i}: not decided, but it lacks its three buttons and its note")
        if e["status"] != "not decided" and (radios(body) or "<textarea" in body):
            bad.append(f"{i}: {e['status']}, but it has buttons or a note")
        rows = re.findall(r'<p class="set(?: removes)?"([^>]*)>(.*?)</p>', body, re.S)
        if e["status"] != "not decided":
            if rows: bad.append(f"{i}: {e['status']}, but it shows statements a yes would set or remove")
            continue
        attr = lambda t: html_text(t).replace('"', "&quot;")
        sets = [(a, b) for a, b in rows if "data-removes" not in a]
        if len(sets) != len(e["sets"]):
            bad.append(f"{i}: a yes sets {len(e['sets'])} statements, the page shows {len(sets)}")
        for (a, b), x in zip(sets, e["sets"]):
            if f'data-in="{attr(x["in"])}"' not in a or f'data-name="{attr(x["name"])}"' not in a \
                    or html_text(x["text"]) not in b or html_text(x["name"]) not in b or html_text(x["in"]) not in b:
                bad.append(f"{i}: what a yes sets in {x['in']}, {x['name']}, is not on the page as plan/ holds it")
        removes = [re.search(r'data-removes="([^"]*)"', a).group(1) for a, _ in rows if "data-removes" in a]
        if removes != [attr(r) for r in e["removes"]]:
            bad.append(f"{i}: a yes removes {e['removes']}, the page shows {removes}")
if want["clear"] and "clear" in sections and items(sections["clear"]) != len(narr["scenes"]):
    bad.append(f"Was it clear?: {items(sections['clear'])} lines for {len(narr['scenes'])} scenes")
if want["test"] and "test" in sections:
    t = sections["test"]
    if C["test"]["question"].replace("'", "&#39;") not in t.replace("'", "&#39;") and C["test"]["question"] not in t:
        bad.append("Test the picture: the question is missing")
    if not re.search(r'<p class="given" hidden', t): bad.append("Test the picture: the answer is not hidden before the button")
    if "Show the answer" not in t: bad.append("Test the picture: the button that shows the answer is missing")
if want["wrong"] and "wrong" in sections and "<textarea" not in sections["wrong"]:
    bad.append("Seen this go wrong?: the field is missing")
if want["choice"] and "choice" in sections:
    c, n = sections["choice"], len(C["choice"]["options"])
    if radios(c) != n: bad.append(f"A choice: {radios(c)} options on the page, {n} in film.json")
    recs = sum(1 for o in C["choice"]["options"] if o.get("recommended"))
    if c.count(">Recommended<") != recs: bad.append(f"A choice: {c.count('>Recommended<')} marked Recommended, {recs} in film.json")
if want["words"] and "words" in sections and sections["words"].count("<dt>") != len(C["words"]):
    bad.append("Words: the count differs from film.json")
if bad:
    print("FAIL\n  " + "\n  ".join(bad))
    sys.exit(1)
print(f"ok: a page that {kind}, with " + ", ".join(n for n, on in want.items() if on))
