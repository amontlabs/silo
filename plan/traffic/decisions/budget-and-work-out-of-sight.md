# How a budget counts work that runs after the call

## The choice

An agent's part, 25 November 2026, from `research/2026-11-25-spend-budget-edges`. Main has a
budget of $2. An agent writes a script that sends 400 requests to OpenAI's Batch API, each with
up to 4,000 tokens of output. The batch is accepted in one request and runs at OpenAI for up to a
day. Its cost exists only in the results file, hours later. The $453 bill in #351 came from a
batch runner.

The same shape has three other forms: an OpenAI response sent with `background: true`, which
answers "queued" at once and is fetched later by id; Anthropic's Message Batches; and Gemini's
explicit cache, billed by the hour for as long as it is kept. Measured against stand-in providers
that bill each of them, a boundary that counts only the calls it watches counted nothing: $1.06 of
background responses, $4.47 of batches and $5.52 of cache storage went by at $0.

The options:

- Hold the bound (recommended). The work leaves only if its whole bound fits: each request's input
  at the dearest rate and its whole output limit, at the batch price, or a cache's storage for its
  whole lifetime. It holds that bound until its result passes Silo, then counts at what the
  provider reports. In the runs, the batch of 400 was refused with "this batch of 400 requests may
  cost up to $8.82, which it holds until its results are read, and this computer has $1.81 left of
  $2.00"; a cache kept for a day was refused at $6.48; batches and background responses whose
  results were fetched were counted at the bill to the cent. It costs a budget that looks spent
  before it is, when the bound is far above the real cost, and a hold that never ends when the
  results are fetched outside Silo (the app shows it, and you release it).
- Deny such work under a budget. Simple and exact. It costs batch pricing, which is half the
  price, for anyone with a budget, and every background response.
- Let it pass uncounted, and say so. It costs the limit: a budget that a batch gets around is not
  a limit, the same reason `what-a-budget-promises` denies a model with no price.

Recommended: hold the bound. It keeps half-price batches usable under a budget, and it never lets
spend pass that the budget did not hold room for.

What the boundary must read for it, all built in the research: OpenAI's `/v1/files` upload with
purpose `batch` (the requests), `/v1/batches` (the start), the batch object's `output_file_id`,
and the results file; Anthropic's `/v1/messages/batches` and its `results`; a response's
`status`, and `GET /v1/responses/{id}` and its `cancel`; Gemini's `cachedContents` create, PATCH
of its `ttl`, and DELETE. A batch whose file was uploaded before Silo, or by another machine, has
no bound: under a budget it is refused.

Not verified: Gemini's storage price per million tokens per hour (the run used $4.50 as a
stand-in), whether OpenAI bills a cancelled background response for what it generated, and
Gemini's batch result format, which was not built.

## Where the drafts of 25 November disagree (integration)

An agent's part, 25 November 2026, written while merging every draft into one branch (`research/2026-11-25-integration-2/`).

#378 proposes refusing batch routes under a budget, "until they are counted", as the gateway tool it studied does. #385 counted them: a batch, a background response or a cache leaves when its whole bound fits, and the count matched the bill to the cent. The condition of #378's proposal is met, so its refusal is the fallback only for work Silo still cannot count, such as code execution billed by container time.

## Two prices checked (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-model-prices`.

- Gemini's cache storage price is on Google's page: $4.50 per million tokens per hour for Gemini
  3.1 Pro Preview, $8.10 at the priority tier, and $0.50 for Gemini 3.8 Flash. The stand-in value
  of $4.50 above is the real one for that model.
- OpenAI now prices eligible background requests 30% below standard for a 2-hour completion
  window and 40% below for an 8-hour one. A bound at the standard price stays a bound for them.
