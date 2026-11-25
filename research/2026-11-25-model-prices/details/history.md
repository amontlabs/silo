# How often prices change, measured

The scripts are in `measure/`; `measure/measure.sh <work dir>` repeats everything. The raw output
is in `results/history.txt` and `results/catalogue.txt`.

## New models

From OpenRouter's catalogue on 25 November 2026, counting text models of Anthropic, OpenAI and
Google by the day OpenRouter listed them (for these providers, the launch day or the day after):

- 43 new models in the last six months, 1.7 a week. September 2026 alone had 12.
- Anthropic: Fable 5 (9 June), Sonnet 5 (30 June), Opus 5 (24 July), Fable 5.1 (1 September), Opus
  5.5 (22 September), Sonnet 5.5 (28 September), Haiku 5.5 (7 October), Fable 5.2 (9 November).

LiteLLM's public table had a price for a new model on the day OpenRouter listed it for half of 68
models since June 2025, within 3 days for 9 in 10, and within 149 days at worst (two Gemma models).
26 others never got a row, most of them image models.

## Price changes of a model that already had a price

LiteLLM's table is a JSON file in a public repository, changed by pull requests. Its first-parent
history has 2,074 versions since September 2023. Each change of an input, output, cache or
long-context price of an Anthropic, OpenAI or Gemini API row was grouped by commit and labelled by
reading the commit and, where it could be found, the provider's page (`measure/labels.json`):

| Year | The provider changed it | The table had it wrong |
|---|---|---|
| 2024 | 2 | 4 |
| 2025 | 4 | 9 |
| 2026 | 8 | 7 |

The provider's changes of 2026 were promotions and introductory prices (Sonnet 5, Gemini 3.6
Flash, GPT-5.6 Sol, GPT-6 Rosalind), OpenAI repricing GPT-5.6 Terra and Luna, and the
`gemini-*-latest` aliases moving to newer models. Three of them are known only from the table's
commits (marked `?` in the labels).

The table's own mistakes: 21 since 2023. The wrong price had been in the table for a median of 19
days, a quarter of them for more than 142 days, and two years at most (`gpt-3.5-turbo`, 3x). They
were off by a median of 2.5 times. Most were cache prices: Gemini cache reads were 2.5 times too
high on several models for 6 to 15 months.

Provider changes reached the table late too: Gemini 1.5 Flash's cut 28 days after it, the
`gpt-4o` alias move 41 days after, Claude 3.5 Haiku's launch price 114 days after, and the
`gemini-*-latest` alias moves 7 to 12 months after.

## Prices announced with a date, on the pages today

- Gemini 4 Carbon: $2 and $10 per million tokens through 30 November 2026, $4 and $20 from
  1 December, on every tier.
- Gemini 3.8 Flash: $0.75 and $3.75 until $1.50 and $7.50 from 1 January 2027.
- GPT-5.6 Sol's promotion ends on 30 November 2026; OpenAI's Ultrafast tier has a promotion too.
- From 3 December 2026, `gemini-omni-1.1-flash` requests are served and billed as
  `gemini-omni-1.2-flash`.
- A dated price can be withdrawn: Anthropic announced Sonnet 5 at an introductory $2 and $10 until
  31 August 2026, then made it the standard price and cancelled the increase.

## What a guess for an unknown model would have done

Of 100 models released since January 2025 (OpenRouter's catalogue):

- Priced like the previous model of its family, 19 of 55 would have been counted too low, by up
  to 8 times (`gemini-3.5-flash` against `gemini-2.5-flash`: 5 times).
- Priced at the dearest model its provider already sold, 7 would have been counted too low, by up
  to 1.6 times. One more, `gemini-2.5-pro-preview`, shows 15 times only because Google's older
  dearer models have left the catalogue. A margin of 2 covered every other one.
