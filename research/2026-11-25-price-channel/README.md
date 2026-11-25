# Where the price table comes from, against what Silo sends home

A trial, not to be merged. Stacked by merge on #360 and #396, whose entries it edits.

#396 recommends that the app fetch Silo's signed price table (`where-prices-come-from`). #360
recommends that Silo send nothing home but a version check, which carries the version
(`what-silo-sends-home`). The two cannot both hold as written. This research finds one request
that does both and carries nothing, a second key so that a stolen release key cannot lower prices,
and what a table that is not current does to a budget.

## What was found

- **One request serves both, and needs no version.** One signed file, the index, names the latest
  version of Silo and the latest table with its hash. The app fetches it once a day at a random
  minute and compares versions itself. Two installs at different versions, with different tables
  and histories, sent byte-identical requests: the path, `User-Agent: Silo` and three fixed
  headers. curl, Python and Node each send more by default (`results/requests.txt`); macOS's
  `URLSession` names the app's build and Darwin's version unless told not to (not measured: no
  Mac).
- **Where it is hosted decides who learns of it.** A static file on GitHub Pages or in the public
  repository gives Amont Labs no log of requests. A server or CDN of its own logs each address and
  time; a GitHub release asset shows a public download count. Mirrors cost nothing in trust, since
  every file is checked by signature wherever it comes from (test: the first address down, the
  second served).
- **One key is not enough.** #396's boundary checks one signature and a rising version. With that
  key, a table that prices Opus 5.5's output at a hundredth was taken. With a floor, a cut below
  half of a price the app holds needs the offline root key too; it was refused without it and
  taken with it. Promotions at half price, a withdrawn dated increase, a removed row (the bound
  applies), raises and new models need only the daily key.
- **The root key would sign about twice a year.** Of the providers' 14 price changes since 2024
  (#396's history), 4 cuts went deeper than half: Gemini 1.5 Flash, o1-mini, Claude 3.5 Haiku
  (direction a guess) and o3 (`results/replay.txt`). Until it signs, the higher price counts, so a
  budget stops early.
- **New models cannot be policed.** 6 of 100 models released since 2025 launched under half the
  cheapest price their provider then sold, Haiku 5.5 at a tenth (`results/new-models.txt`). A rule
  on a new row's price would refuse real launches, so a stolen daily key can price a model the app
  has no row for. That is the residual; the root key's list of daily keys, with end dates, bounds
  how long it lasts.
- **A frozen table mostly errs high.** Of the 14 changes, a table frozen before each counted 4 high,
  4 right (dated rows), 2 needed no new price (aliases), 1 low (Gemini 2.5 Flash at general release)
  and 3 in a direction not known. The index expires after two days, so the app tells "nothing new"
  from "unreachable or shown an old index".
- **Small.** The table is 54 KB, 2.3 KB gzipped; the index 307 bytes (`results/sizes.txt`). The
  app fetches a whole table, never a difference, which would name the version a Mac holds.
- **A known failure of the public alternative.** LiteLLM fetches its table from GitHub at every
  start and falls back to the copy in its package; in January 2026 a malformed entry on its main
  branch made every install fall back silently to a stale table
  ([incident report](https://docs.litellm.ai/blog/model-cost-map-incident)). Here a bad file is
  refused, recorded, and the last good table kept.

## What it changes in the plan

- `what-silo-sends-home` (open, from #360): the answer now allows one daily fetch of one public
  signed file that names the latest version of Silo and of the table, carrying nothing, not even
  the version; its option is edited to match. Notes: the measurement, the hosting, what the answer
  leaves to `recipe-audit`, `computer-image` and `claude-code-fetched-by-recipe`, and a property
  for polarzero to word.
- `where-prices-come-from` (open, from #396): the answer now has two keys and the floor, the
  shared index, files brought by hand, a team's own signed table, and the fetch turned off. It
  `needs` `what-silo-sends-home`. Notes: the tests, the replay, the residual.
- Challenges, for polarzero: #360's version check need not carry the version; #396's single key
  lets a stolen key lower every price.

No verdict was written.

## What was built

| Path | What |
|---|---|
| `channel/client.py` | The app's side: root files rotated one version at a time (old and new root must both sign), the index (daily key, not expired, version not lower), the table (hash and version named by the index, the floor), files by hand, your own and a team's prices, the fetch turned off, the next fetch at a random minute. Every event goes to a log, as the record would keep it. |
| `channel/cuts.py` | What a table lowers against the one the app holds: per provider, model and tier, today and on each later date of effect, with multipliers, using #396's `pick` (exact id before prefix). |
| `channel/publish.py` | Keys, root files, the index and tables as static files. |
| `channel/sig.py` | #396's signature format, with several lines for a root file. |
| `test/test_channel.py` | 25 tests against a local static server (`results/tests.txt`). |
| `measure/` | `requests.py`, `sizes.py`, `replay.py` with `history.json` (this agent's reading of #396's labels), `new_models.py`. |

## Not verified

- No Mac: `URLSession`'s default headers and TLS session reuse on macOS were not measured.
- That GitHub Pages gives the repository's owner no per-request log rests on GitHub's documentation.
- The direction of 4 of the 14 price changes is a guess, as marked in `measure/history.json`.
- OpenRouter's catalogue lists only models it still sells, so "the cheapest the provider then sold"
  misses retired models.
- The boundary in Zig (#396's `prices.zig`) was not changed: it still checks one key. Zig is not
  installed in this container.
- A root key on a hardware key, and how polarzero signs with it, were not tried.

## Running it

```
python3 -m unittest discover -s test -v
python3 measure/requests.py
python3 measure/sizes.py ../2026-11-25-model-prices/table/prices-2026112501.json
python3 measure/replay.py measure/history.json
curl -s https://openrouter.ai/api/v1/models -o /tmp/models.json && python3 measure/new_models.py /tmp/models.json 2025-01-01
```
