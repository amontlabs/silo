"""check_player.py <film dir> <voice id> < dump: reads the page's DOM after build (from --dump-dom) and fails on
an error, on a layout note from the kit, or on a scene shorter than its narration in that voice."""
import html, json, pathlib, re, sys
import place
ch, vid = place.film(sys.argv[1]), sys.argv[2]
dom = sys.stdin.read()
voice = next(v for v in json.loads((ch / "voice.json").read_text())["voices"] if v["id"] == vid)
bad = []
body = re.search(r"<body([^>]*)>", dom)
broken = body and "broken" in (re.search(r'class="([^"]*)"', body.group(1)) or [None, ""])[1]
err = re.search(r'<pre id="errors">(.*?)</pre>', dom, re.S)
text = html.unescape(err.group(1)).strip() if err else ""
if broken:
    bad.append("the player reports errors:\n" + text)
elif text:
    bad.append("the kit's layout notes:\n" + text)
t = re.search(r'data-timing="([^"]*)"', dom)
if not t:
    bad.append("no timing: build() did not finish")
    lengths = []
else:
    timing = json.loads(html.unescape(t.group(1)))
    starts = [s[0] for s in timing["scenes"]]
    lengths = [b - a for a, b in zip(starts, starts[1:] + [timing["total"]])]
    for i, (L, sc) in enumerate(zip(lengths, voice["scenes"])):
        if L + 0.01 < sc["total"]:
            bad.append(f"scene {i + 1} lasts {L:.2f} s, its narration {sc['total']:.2f} s")
    if len(lengths) != len(voice["scenes"]):
        bad.append(f"the film has {len(lengths)} scenes, the narration {len(voice['scenes'])}")
print(f"{vid}: scenes " + "  ".join(f"{L:.1f}" for L in lengths) + f"  total {sum(lengths):.1f} s"
      + "   (narration " + "  ".join(f"{s['total']:.1f}" for s in voice["scenes"]) + ")")
if bad:
    print("FAIL\n  " + "\n  ".join(bad))
    sys.exit(1)
print("ok: no errors, no layout notes, every scene at least as long as its narration")
