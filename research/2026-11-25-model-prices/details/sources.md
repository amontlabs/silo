# Where each provider's prices can be read

Read on 25 November 2026. "Machine-readable" means a file a program can read without scraping
HTML. Every page below also has a markdown copy at a stable address, which `update/fetch.py` reads.

| Provider | Source | Machine-readable | What it gives | Per call in the reply |
|---|---|---|---|---|
| Anthropic | [pricing page](https://platform.claude.com/docs/en/about-claude/pricing.md) | markdown tables | every model: input, 5-minute and 1-hour cache writes, cache hits, output; batch; fast mode; Haiku 5.5 over 100,000 tokens; US-only inference at 1.1x for Claude 4.6 and later | token counts, `speed`; no cost |
| Anthropic | Models API (`/v1/models`) | JSON | ids and capabilities; no price | |
| Anthropic | Usage and Cost API | JSON, administrator key | cost by the day, usage down to the minute | |
| OpenAI | [pricing page](https://developers.openai.com/api/docs/pricing.md) | markdown tables | standard, batch, flex, fast (Priority was renamed Fast on 30 July 2026), ultrafast; short and long context; 30-minute and 12-hour cache writes; promotions with end dates | token counts, `service_tier`; no cost |
| Google | [Gemini API pricing page](https://ai.google.dev/gemini-api/docs/pricing.md.txt) | markdown with tags | standard, batch, flex, priority tabs per model; prompts over 200,000 tokens; cache storage per hour; dated prices ("$2.00 through November 30, 2026. $4.00 starting December 1, 2026.") | token counts, `modelVersion`; no cost |
| Bedrock | [AWS Price List](https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AmazonBedrockFoundationModels/current/index.json) | JSON, no key, 16 MB, republished about daily | every Claude model per region: global and regional endpoints, batch, cache reads and writes (5-minute and 1-hour) | |
| Vertex | Cloud Billing Catalog API | JSON, needs a Google Cloud API key | not read here: no key in this container | |
| OpenRouter | [`/api/v1/models`](https://openrouter.ai/api/v1/models) | JSON, no key | 472 models with prompt, completion, cache read and write, web search, long-context overrides, `:batch` variants | `usage.cost` in each reply ([usage accounting](https://openrouter.ai/docs/use-cases/usage-accounting)) |
| LiteLLM | [`model_prices_and_context_window.json`](https://github.com/BerriAI/litellm) | JSON, MIT | thousands of rows, changed by pull requests; no dates of effect | |
| Pydantic `genai-prices` | [`prices/data.json`](https://github.com/pydantic/genai-prices) | JSON and YAML, MIT | dated prices (`start_date`), tiers (`service_tier`, `processing: batch`), a `prices_checked` day per model, a note of where each price was read | |

## What changes the price of one call

Each of these is in the request or the reply, and the boundary has to read it:

| What | Where Silo reads it | Factor | Built in the research |
|---|---|---|---|
| Batch | the batch endpoints (#385) | 0.5x | yes (#385) |
| Anthropic fast mode | `speed: "fast"`, `usage.speed` | 2x on Opus 5.5 and Fable 5.2 | yes (#374) |
| OpenAI fast, flex | `service_tier` in request and reply | 2x, 0.5x | yes (#374) |
| Gemini flex, priority | not in #374's boundary | 0.5x, 1.8x | no |
| Long prompts | the call's input count | Gemini over 200,000 tokens, OpenAI over 272,000, Haiku 5.5 over 100,000 | yes (#374) |
| Anthropic US-only inference | `inference_geo: "us"` in the request | 1.1x, Claude 4.6 and later | yes, here |
| Bedrock and Google Cloud regional endpoints | the host or model id the program calls | 1.1x | no: Bedrock traffic is not read by the boundary |
| OpenAI regional endpoints | the host | 1.1x for models released on or after 5 March 2026 | no |
| OpenAI background with a completion window | the request | 0.7x (2 hours) or 0.6x (8 hours) | no: a bound at 1x stays a bound |
| Promotions and introductory prices | a date | varies | yes: rows with `from` |
| Aliases (`gemini-flash-latest`) and redirects (`gemini-omni-1.1-flash` to 1.2 from 3 December 2026) | the model the reply names | the target model's price | yes (#374 prices the reply's model) |
| Negotiated discounts | your own prices file | yours | yes, here |
| Free allowances (Gemini: 5,000 searches a month) | nothing in the traffic | | no: Silo counts every search |
