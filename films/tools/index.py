#!/usr/bin/env python3
"""index.py: writes index.html, the one page that lists the films in the order to watch them.

Run it from films/ after a film is built or rebuilt:
    python3 -I tools/index.py
It reads each film's film.json (title, line, kind) and voice.json (length), and links to
<id>/out/index.html. A film with no built page is left out and named on stderr.
"""
import html
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# The films, in the order they are watched.
ORDER = ["engine-one-call", "what-silo-knows", "actions-and-kinds", "rules-and-precautions", "added-classifiers"]
# Films that belong to no stage of the order. They are listed apart.
BACKGROUND = []

# What the owner does on each kind of page, in a few words.
KIND = {"teaches": "explains", "decides": "you decide"}


def film(film_id):
    folder = ROOT / film_id
    page = folder / "out" / "index.html"
    if not page.exists():
        print(f"left out, not built: {film_id}", file=sys.stderr)
        return None
    f = json.loads((folder / "film.json").read_text())
    voice = json.loads((folder / "voice.json").read_text())
    seconds = round(voice["voices"][0]["total"])
    scenes = len(voice["voices"][0]["scenes"])
    return {
        "href": f"{film_id}/out/index.html",
        "title": f["title"],
        "line": f["line"],
        "kind": KIND.get(f["kind"], f["kind"]),
        "length": f"{seconds // 60} min {seconds % 60:02d}" if seconds >= 60 else f"{seconds} s",
        "scenes": scenes,
    }


def rows(ids, numbered):
    out = []
    for n, film_id in enumerate(ids, start=1):
        f = film(film_id)
        if f is None:
            continue
        number = f'<span class="n">{n}</span>' if numbered else '<span class="n"></span>'
        out.append(
            f'<a class="film" href="{html.escape(f["href"])}">{number}'
            f'<span class="what"><span class="title">{html.escape(f["title"])}</span>'
            f'<span class="line">{html.escape(f["line"])}</span></span>'
            f'<span class="meta"><span class="kind">{html.escape(f["kind"])}</span>'
            f'<span>{f["scenes"]} scenes · {f["length"]}</span></span></a>'
        )
    return "\n".join(out)


PAGE = """<!doctype html>
<html lang="en">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Silo: the films</title>
<style>
  :root {{ --bg: #f4f5f7; --card: #ffffff; --ink: #14171c; --muted: #5d6673; --line: #dfe3e8; --accent: #2f5fd0; --you: #6a58d8; }}
  @media (prefers-color-scheme: dark) {{
    :root {{ --bg: #0f1216; --card: #171b21; --ink: #e9edf2; --muted: #98a2b0; --line: #29303a; --accent: #7aa2ff; --you: #a096eb; }}
  }}
  * {{ box-sizing: border-box; }}
  body {{ margin: 0; background: var(--bg); color: var(--ink); font: 16px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; }}
  main {{ max-width: 820px; margin: 0 auto; padding: 48px 16px 72px; }}
  h1 {{ font-size: 28px; font-weight: 500; letter-spacing: -0.02em; margin: 0 0 4px; }}
  .sub {{ color: var(--muted); font-size: 18px; margin: 0 0 28px; }}
  h2 {{ font-size: 14px; font-weight: 500; color: var(--muted); margin: 36px 0 10px; }}
  .film {{ display: grid; grid-template-columns: 34px 1fr auto; gap: 14px; align-items: center; padding: 16px 18px; margin: 0 0 10px;
    background: var(--card); border: 1px solid var(--line); border-radius: 12px; color: inherit; text-decoration: none; }}
  .film:hover, .film:focus-visible {{ border-color: var(--accent); outline: none; }}
  .n {{ font-size: 20px; color: var(--muted); font-variant-numeric: tabular-nums; text-align: center; }}
  .what {{ display: grid; gap: 2px; min-width: 0; }}
  .title {{ font-size: 19px; font-weight: 500; }}
  .line {{ color: var(--muted); text-wrap: balance; }}
  .meta {{ display: grid; gap: 4px; justify-items: end; color: var(--muted); font-size: 14px; white-space: nowrap; }}
  .kind {{ color: var(--you); border: 1px solid color-mix(in srgb, var(--you) 45%, transparent); border-radius: 999px; padding: 1px 10px; }}
  @media (max-width: 560px) {{ .film {{ grid-template-columns: 26px 1fr; }} .meta {{ grid-column: 2; justify-items: start; grid-auto-flow: column; gap: 10px; }} }}
</style>
<main>
  <h1>Silo: the films</h1>
  <p class="sub">Watch them in this order.</p>
{films}
{background}
</main>
</html>
"""


def main():
    films = rows(ORDER, numbered=True)
    back = rows(BACKGROUND, numbered=False)
    background = f"  <h2>Background, in any order</h2>\n{back}" if back else ""
    (ROOT / "index.html").write_text(PAGE.format(films=films, background=background))
    print(ROOT / "index.html")


if __name__ == "__main__":
    main()
