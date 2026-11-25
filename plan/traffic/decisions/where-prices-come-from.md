# Where prices come from

Spend and budgets are counted at Silo's price for each model (`what-traffic-gives`,
`how-spend-is-shown`, `spend-per-conversation`). No decision said where those prices come from,
how a new price reaches the app, or what happens when Silo's table is wrong. This entry asks it.
What happens to a call to a model that is in no table is for `what-a-budget-promises`.

## The choice

An agent's part, 25 November 2026, from `research/2026-11-25-model-prices`. Google's price page
says Gemini 4 Carbon costs $2 per million input tokens and $10 per million output tokens through
30 November 2026, and $4 and $20 from 1 December. Main has a budget of $50 and an agent that uses
Gemini 4 Carbon. On 1 December, a table without that second price counts half of what Google
bills, and the budget of $50 lets $100 go.

Prices move often enough for this to matter. Anthropic, OpenAI and Google listed 43 new models in
the last six months, 1.7 a week. In 2026, LiteLLM's public table changed 8 prices of these three
providers because the provider changed them: promotions that start or end on a date, an
introductory price that was later made permanent, and aliases such as `gemini-flash-latest`
moved to a newer model. It also fixed 7 of its own mistakes. Since 2023 it fixed 21; the wrong
price had been there for a median of 19 days, a quarter of them for more than 142 days, and was off
by a median of 2.5 times.

The options:

- Silo's own table, signed and fetched (recommended). A tool reads Anthropic's, OpenAI's and
  Google's price pages, which each give a markdown copy at a stable address, and builds the next
  release. It compares every row with OpenRouter's catalogue, which resells the same models at the
  provider's price, and stops on any difference until it is settled. The release is signed with
  Silo's key, and the app takes it only if the signature holds and its version is above the last
  one taken. It costs a pipeline Silo runs every day, a signing key to keep, and the app reaching
  one Silo address for the table.
- A table inside each Silo release only. Nothing is fetched. It costs a wait for an app update for
  every new model and every price change, and a budgeted computer cannot use a model released
  since.
- A public table, fetched as it is (LiteLLM's or Pydantic's `genai-prices`). Nothing to maintain.
  It costs trust in a file anyone can change by a merged pull request, with its mistakes: the
  measured ones above sat for weeks to months.
- The provider's own figure for each call. Exact where it exists. OpenRouter puts the cost of each
  call in its reply. Anthropic, OpenAI and Google do not. Anthropic's cost report needs an
  administrator's key and gives totals by the day, too late to hold a budget. It costs every
  provider but OpenRouter.

Recommended: Silo's own table, signed and fetched. A budget needs the price before the call, and
only this option gives it the day a price changes without trusting a file that anyone can edit.

## What the research built and measured

In `research/2026-11-25-model-prices`, on #374's boundary:

- The tool read 167 rows of text models from the three pages on 25 November 2026, with every
  tier (batch, flex, priority or fast), long-context prices, and dated prices such as Gemini 4
  Carbon's. #374's table, copied by hand the day before, matched the pages on all 13 rows they
  share.
- The comparison with OpenRouter found 4 differences. One is a rounding on OpenAI's page: it shows
  GPT-5.4's batch cache read as $0.13 where the price is $0.125. Another is OpenRouter's: it
  charges 10% more than Google's page for Gemini 3.8 Flash. Claude on Bedrock, read from AWS's own
  price list, matched Anthropic's page on every row, and its regional endpoints cost 1.1 times the
  global one.
- The boundary took a signed release, refused a copy edited by hand (bad signature), took the next
  release with a new model, and refused an older signed release put back (its version was not
  above). The version it took is remembered across restarts.
- A request with `inference_geo: "us"` was counted at 1.1 times the price, as Anthropic's page
  says for Claude 4.6 and later.
- Your own prices, in a file beside the table, won over it; a discount you write there applies to
  every row of that provider.

## What a release holds

`format` 2, an increasing `version`, `published`, `check_by` (14 days later), the rows of #374's
table with `source` and `checked` on each, and `multipliers` (such as 1.1 for US-only inference).
A signature file beside it holds one line: the key's id and the Ed25519 signature of the file's
exact bytes.

## Not decided here

- Whether a discount you negotiated is counted, or spend stays at list price as
  `how-spend-is-shown` proposes. The table can hold either; the notes of `how-spend-is-shown` say
  more.
- How the app shows that the table is past its check-by day. The boundary goes on using it.
- OpenAI charges 10% more on regional endpoints for models released on or after 5 March 2026. The
  boundary would need the day each model was released; it was not built.
