# Keeping model prices current for budgets

A trial, not to be merged. Spend and budgets (#374, #385) count dollars from Silo's price table.
This research asks where those prices come from, how often they change, and what a budget does
when the table is wrong or has no row. It builds an answer into #374's boundary and measures it.

## What was found

- **Prices change often, and announced dates matter.** Anthropic, OpenAI and Google listed 43 new
  text models in the last six months, 1.7 a week. Google's page says Gemini 4 Carbon doubles on
  1 December 2026, and Gemini 3.8 Flash on 1 January 2027. A table without dated rows counts half
  the bill from that day. Promotions with end dates, introductory prices and aliases that move
  (`gemini-flash-latest`) made most of 2026's 8 provider price changes.
- **Public tables are wrong for weeks.** LiteLLM's table fixed 21 of its own mistakes on these
  three providers since 2023. The wrong price had sat a median of 19 days, a quarter for more than
  142, and was off by a median of 2.5 times. A dated increase can also be withdrawn: Anthropic
  cancelled Sonnet 5's for 1 September 2026.
- **Each provider's page can be read by a program.** Anthropic's, OpenAI's and Google's pages all
  have a markdown copy at a stable address. `update/fetch.py` read 167 rows of text models from
  them, with every tier and dated price. AWS publishes Claude on Bedrock as JSON, and OpenRouter its
  whole catalogue. No first-party provider gives a call's cost in its reply; OpenRouter does.
- **Two sources catch mistakes one hides.** The pages and OpenRouter differed on 4 rows. One is
  OpenAI's page rounding a $0.125 price to $0.13. One is OpenRouter charging 10% more than Google
  for Gemini 3.8 Flash. Bedrock's global prices matched Anthropic's page on all 12 models, and its
  regional endpoints cost 1.1 times as much. #374's hand-copied table matched the pages on all 13
  shared rows.
- **A signed table works in the boundary.** It took a signed release, refused a copy edited by
  hand, took the next release with a new model, and refused an older signed release put back,
  also after a restart. A request with `inference_geo: "us"` was counted at 1.1 times, as Anthropic
  charges for Claude 4.6 and later.
- **A bound keeps a new model working under a budget.** Counted at twice the dearest price of its
  provider, a model with no row was admitted. A $1 budget stopped it after $0.46 had really been
  spent. Of 100 models released since 2025, 7 cost more than the dearest their provider sold
  before, by 1.6 times at most. Pricing a model like the previous one of its family would have
  counted 19 of 55 too low, by up to 8 times.

Details: [`details/sources.md`](details/sources.md) (each provider's sources and what changes a
call's price) and [`details/history.md`](details/history.md) (the measurements).

## What it changes in the plan

- New open decision `where-prices-come-from`: Silo's own table, signed and fetched (recommended),
  against a table only in app releases, a public table fetched as it is, or the provider's figure
  per call. It proposes the property `prices-signed-or-yours`.
- `what-a-budget-promises`: notes on a third option for a model with no price, counting it at a
  bound, recommended over denying it. The answer is left as it is.
- Notes on `how-spend-is-shown` (US-only and regional prices, negotiated discounts),
  `budget-and-work-out-of-sight` (Gemini's storage price is the real one; OpenAI's discounted
  background windows), `what-traffic-gives` (aliases and redirects are priced by the model that
  answered) and `spend-per-conversation` (its doubt now has an entry).

No verdict was written.

## What was built

In #374's boundary (`research/2026-11-24-spend-budgets/boundary`):

| File | Change |
|---|---|
| `prices.zig` | A release: `format` 2, `version`, `published`, `check_by`, rows with `source` and `checked`, `multipliers`. With `price_keys`, the file is taken only with a valid Ed25519 signature in `<file>.sig` and a version above the one in `<file>.seen`. A failure keeps the last good table and is recorded. `local_prices` holds your own rows and multipliers, which win. `bound()` prices a model with no row. Seven unit tests. |
| `spend.zig` | Reads `inference_geo` from Anthropic requests; config keys `price_keys`, `local_prices`, `unknown_model` ("refuse" or "bound"), `unknown_margin`. |
| `main.zig` | Prices each call with the multipliers it asked for, records each reading of the table, and counts a model with no row at a bound when set to. |

Here:

| Folder | What |
|---|---|
| `update/` | `fetch.py` reads the five sources into rows; `check.py` compares two of them; `build.py` makes the next release and lists disputes; `sign.py` makes a key and signs. |
| `table/` | The release built on 25 November 2026 from the pages: 167 rows. Its 4 disputes are unsettled. |
| `run/` | `release.sh` runs the two end-to-end cases through the boundary and the stand-in providers; `check.py` checks them. |
| `measure/` | The history measurements; `measure.sh` repeats them. |
| `results/` | What each printed on 25 November 2026. |

#385's edge runs `prev_id`, `batch` and `cache_expiry` still pass on this build.

## Not verified

- The pages' prices were not checked against a real bill: this container has no provider key.
- The labels of LiteLLM's price changes are this agent's reading; three are marked as guesses.
- OpenAI's and Google's cost reports were not read again today. Vertex's price catalogue needs a
  Google Cloud key and was not read.
- Gemini's flex and priority tiers, OpenAI's regional uplift, and Bedrock or Vertex traffic are not
  read by the boundary.
- Re-counting calls once a model's price arrives (they stay at the bound) was not built.
- The key handling is a prototype: the private key lives in a file for the run and is deleted.

## Running it

```
cd ../2026-11-24-spend-budgets && run/tls.sh && (cd boundary && zig build && zig build test)
../2026-11-25-model-prices/run/release.sh
python3 update/fetch.py <dir> && python3 update/check.py <dir>/sources.json pages-vs-openrouter
python3 update/build.py <dir>/sources.json - <dir>/release.json
measure/measure.sh <work dir>
```
