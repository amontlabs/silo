#!/usr/bin/env python3
"""build.py <film dir>: writes <film>/out/index.html, one file with everything inlined.

It reads film.json, voice.json (from voice.py) and explainer.js, and the entries the film shows from
plan/ (tools/entries.py). The film folder must be a direct child of W, and every clip a plain
file of its voice/ (tools/place.py). It takes the player from W/player/ and the parts of the kit left unchanged
(fonts, GSAP and its plugins, icons, player.css) from W/kit/kit/. W is films/. The page ends with
the texts of W/kit/licences/ in a comment: the icons' and the fonts' licences ask that their text
goes with every copy.
"""
import _venv  # noqa: F401  (W/.venv, offline)
import base64, html, json, pathlib, sys
import entries, place

W = pathlib.Path(__file__).resolve().parent.parent
KIT, PLAYER = W / "kit" / "kit", W / "player"
LICENCES = W / "kit" / "licences"
VENDOR = ["gsap.min.js", "MotionPathPlugin.min.js", "DrawSVGPlugin.min.js", "MorphSVGPlugin.min.js", "CustomEase.min.js", "icons.js"]
FONTS = {"geist-regular.otf": "font/otf", "geist-medium.otf": "font/otf", "geist-mono.ttf": "font/ttf"}


def js(s):   # safe inside <script>…</script>
    return s.replace("</script", "<\\/script").replace("<!--", "<\\!--")


def data_url(path, mime):
    return f"data:{mime};base64," + base64.b64encode(path.read_bytes()).decode()


def licences():   # every text of kit/licences/, as one HTML comment
    texts = "\n\n".join(f"{p.name}\n\n{p.read_text().strip()}" for p in sorted(LICENCES.glob("*.txt")))
    assert texts and "-->" not in texts and "--!>" not in texts, LICENCES
    return f"<!--\nLicences of the third-party files inlined in this page.\n\n{texts}\n-->\n"


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    ch = place.film(sys.argv[1])
    film = json.loads((ch / "film.json").read_text())
    film["entries"] = entries.shown(film["shows"])
    voice_path = ch / "voice.json"
    if not voice_path.exists():
        sys.exit(f"{voice_path} is missing: run tools/voice.py {ch} first.")
    voice = json.loads(voice_path.read_text())
    for v in voice["voices"]:
        for sc in v["scenes"]:
            for x in sc["sentences"]:
                x["src"] = data_url(place.clip(ch, x["file"]), "audio/mpeg")
    css = (KIT / "player.css").read_text()
    for name, mime in FONTS.items():
        css = css.replace(f'url("fonts/{name}")', f'url("{data_url(KIT / "fonts" / name, mime)}")')
    css += "\n" + (PLAYER / "page.css").read_text()
    vendor = "\n".join(f"<script>{js((KIT / 'vendor' / f).read_text())}</script>" for f in VENDOR)
    player = "\n".join(f"<script>{js((PLAYER / f).read_text())}</script>" for f in ["voice.js", "kit.js"])
    page = (PLAYER / "page.html").read_text()
    parts = {
        "<!--TITLE-->": html.escape(film["title"]),
        "<!--LINE-->": html.escape(film["line"]),
        "<!--STYLE-->": css.replace("</style", "<\\/style"),
        "<!--VENDOR-->": vendor,
        "<!--FILM-->": js(json.dumps(film)),
        "<!--VOICE-->": js(json.dumps(voice)),
        "<!--PLAYER-->": player,
        "<!--PAGE-->": js((PLAYER / "page.js").read_text()),
        "<!--EXPLAINER-->": js((ch / "explainer.js").read_text()),
    }
    for k, v in parts.items():
        assert k in page, k
        page = page.replace(k, v)
    page = page.rstrip("\n") + "\n" + licences()
    out = place.inside(ch, "out")
    out.mkdir(exist_ok=True)
    (out / "index.html").write_text(page)
    print(f"wrote {out / 'index.html'} ({(out / 'index.html').stat().st_size // 1024} KB)")


if __name__ == "__main__":
    main()
