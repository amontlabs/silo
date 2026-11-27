# Spend, usage limits and cost visibility for coding agents: evidence (2024 to Nov 2026)

Researched 2026-11-24. Sources: GitHub issue pages (read through github.com; the `gh` API was blocked in this session, so star counts and issue counts come from the public web pages on 2026-11-24), the Hacker News Algolia API (points and comment counts are exact), the Cursor forum, the OpenAI community forum, and press. Reddit could not be fetched directly, so it appears only through HN cross-posts. Every URL below was opened, except where a line says it came from search results only. All amounts and quotes are as the reporter states them; most are one person's account and were not independently confirmed.

Note on model names: posts from mid-2026 onward mention models such as Opus 5 / 5.5, Fable 5.x, Mythos 5, GPT-5.6 Sol and GPT-6 Astra. They are recorded as written.

---

## 1. Popularity of homemade trackers (signal of the pain)

Stars and forks as shown on github.com on 2026-11-24.

| Tool | Stars | Forks | What it does | URL |
|---|---|---|---|---|
| rtk-ai/rtk | 87.7k | 5.6k | A CLI proxy that cuts the tokens dev commands feed the model by 60 to 90% | https://github.com/rtk-ai/rtk |
| BerriAI/litellm | 63.8k | 13.5k | A gateway with spend tracking and per-key, per-user budgets | https://github.com/BerriAI/litellm |
| steipete/CodexBar | 23.3k | 2.2k | A menu bar showing Codex and Claude quota windows and reset countdowns, without login (later 100+ providers) | https://github.com/steipete/CodexBar |
| ryoppippi/ccusage | 19.6k | 936 | Reads local agent logs to report tokens and cost; now supports 20 agents | https://github.com/ryoppippi/ccusage |
| sirmalloc/ccstatusline | 13.7k | 614 | A status line with session cost, 5h block forecast, weekly usage, extra-usage budget and cache hit rate | https://github.com/sirmalloc/ccstatusline |
| AgentSeal/codeburn | 11.9k | 911 | Cost by task category and by subagent, across 37+ tools | https://github.com/AgentSeal/codeburn |
| Maciek-roboblog/Claude-Code-Usage-Monitor | 8.8k | 460 | A real-time burn rate that predicts when you hit the cap | https://github.com/Maciek-roboblog/Claude-Code-Usage-Monitor |
| Helicone/helicone | 6.3k | 709 | LLM observability through a proxy | https://github.com/Helicone/helicone |
| junhoyeo/tokscale | 5.9k | 485 | Tokens across agents, plus a global leaderboard | https://github.com/junhoyeo/tokscale |
| chiphuyen/sniffly | 1.3k | 118 | A Claude Code dashboard with usage stats and error analysis | https://github.com/chiphuyen/sniffly |
| snipeship/ccflare | 1.1k | 148 | A Claude Code proxy that load-balances across accounts | https://github.com/snipeship/ccflare |

HN launches of trackers: Claude Code Usage Monitor got 245 points and 135 comments (2025-06-19, https://news.ycombinator.com/item?id=44317012); CodeBurn got 112 points and 27 comments (2026-04-13, https://news.ycombinator.com/item?id=47759035); ccusage got 75 points and 30 comments (2025-07-18, https://news.ycombinator.com/item?id=44610925). Smaller ones keep appearing:
- Claude Meter: https://news.ycombinator.com/item?id=46956460
- Claumon (forecasting): https://news.ycombinator.com/item?id=48488753
- CC-Ledger (per-session and per-PR): https://news.ycombinator.com/item?id=48112700
- Tokenyst ("stop getting shocked by Claude Code API bills"): https://news.ycombinator.com/item?id=48095283
- Vigilo: https://news.ycombinator.com/item?id=47144737
- AI Gauge: https://news.ycombinator.com/item?id=48392849
- Hydra ("never stop coding when your AI CLI hits a rate limit"): https://news.ycombinator.com/item?id=47851935
- LimitPing: https://news.ycombinator.com/item?id=48434613
- Reset alerts by text: https://news.ycombinator.com/item?id=49896361
- Tarmac ("know what Claude Code will cost before you run it"): https://news.ycombinator.com/item?id=47267782
- Budget kill switches:
  - AgentGuard, 47 points: https://news.ycombinator.com/item?id=44742710
  - AgentBudget: https://news.ycombinator.com/item?id=47418574
  - AgentFuse: https://news.ycombinator.com/item?id=46404312
  - TokenFence: https://news.ycombinator.com/item?id=47551568
  - Compass: https://news.ycombinator.com/item?id=48623314
  - MartinLoop: https://news.ycombinator.com/item?id=48098126
  - Shotoku: https://news.ycombinator.com/item?id=50496705
  - Guardian Runtime: https://news.ycombinator.com/item?id=48503911

**Reading:** the token-reduction proxy (rtk, 87.7k) and the quota-window menu bar (CodexBar, 23.3k) have far more stars than cost reports (ccusage, 19.6k). People most want to *stretch* and *see* their subscription windows; dollar reports come second. About a dozen kill-switch projects launched, and each got little traction. Most monkey-patch the SDK, and HN commenters distrust that ("You install it and it might be doing nothing", AgentGuard thread).

## 2. Recurring complaint counts

GitHub issue search counts are from the repository issue pages on 2026-11-24. Keyword searches match words loosely, so they are upper bounds. Label counts are exact.

| Repo | Query | Total |
|---|---|---|
| anthropics/claude-code | `is:issue label:area:cost` | **3,613** |
| anthropics/claude-code | `is:issue "usage limit"` | 2,317 |
| anthropics/claude-code | `is:issue weekly limit` | 1,851 |
| anthropics/claude-code | `is:issue budget OR "spend limit" OR "cost limit"` | 2,607 (loose) |
| anthropics/claude-code | `is:issue compaction loop` | 681 (loose) |
| anthropics/claude-code | `is:issue silently switched model` | 462 (loose; page 1 had about 10 genuine silent model switches in one month) |
| anthropics/claude-code | `is:issue "per session" cost limit OR "max cost" OR "budget per"` | 412 |
| openai/codex | `is:issue usage limit` | **3,364** |
| cline/cline | `is:issue cost` | 266 |
| RooCodeInc/Roo-Code | `is:issue cost OR budget` | 282 (activity stops around May 2026) |
| Aider-AI/aider | `is:issue cost OR budget OR "spend"` | 414 (mostly unrelated matches) |

On 2026-11-22 to 24, page 1 of claude-code `"usage limit"` held 25 issues opened in those 3 days. Most were "limit drains faster than my logs explain".

Hacker News (Algolia, stories since 2024-01-01, nbHits):

| Query | Hits |
|---|---|
| claude code cost | 578 |
| claude code usage | 539 |
| codex rate limit | 323 |
| codex usage limits | 303 |
| cursor charges | 254 |
| spend limit | 250 |
| token usage claude code | 210 |
| claude code usage limits | 162 |
| ccusage | 159 |
| weekly limit | 127 |
| cursor bill | 116 |
| runaway agent cost | 20 |

---

## 3. Items

Format: **summary**, then source, date, agent, amount or impact, quote, engagement, and the fix people used.

### A. Runaway bills and spend limits that do not stop

1. **The $1 spend limit was ignored: 5 auto-recharges added up to $4,349.73 in 26 minutes.**
   - **Source:** https://github.com/anthropics/claude-code/issues/110959
   - **Date and agent:** 2026-11-18, Claude Code with a Console/API key.
   - **Impact:** the billing page showed "$4,029.05 spent, 402,905% of $1 limit", while the usage export showed $22.13. Auto-recharge had turned itself back on.
   - **Quote:** Anthropic's email said "We found signs that the API key(s) below ... may have been exposed or used without your authorization."
   - **Engagement:** 2 👍 and 1 👀; labels `area:cost`, bug.
   - **Fix:** a community member suggested a card chargeback. Support called the charges "prepaid purchases" with no refund.

2. **A Stop-hook loop in headless runs cost about $9,364 in 6 days; the issue names the missing per-session ceiling.**
   - **Source:** https://github.com/anthropics/claude-code/issues/107256
   - **Date and agent:** 2026-11-04, Claude Code `claude -p` on a schedule.
   - **Impact:** about 2.4B tokens, mostly cache reads; about 10,000 refused Edits in the worst session; $15,066.79 in auto-reloads that week.
   - **Quote:** "There is no per session or per run spend ceiling. The monthly spend limit is the only control"
   - **Engagement:** 8 comments from 5 people.
   - **Fix:** the user's own breaker after 3 Stop blocks, plus `--max-turns`, `--max-budget-usd` and auto-reload turned off for scheduled runs.

3. **A $37,901.73 AWS Bedrock bill from a prompt-caching miss through LiteLLM.**
   - **Source:** https://news.ycombinator.com/item?id=47933355
   - **Date and agent:** 2026-04-28, Droid → LiteLLM → Bedrock → Opus 4.6.
   - **Impact:** about 6.47B uncached input tokens cost about $35.6k; cache reads were only about $918.
   - **Quote:** "“Budget alerts are configured” is not the same as “spend will stop.”"
   - **Engagement:** 8 points.
   - **Want:** "this workflow may not send more than N uncached input tokens/hour", "stop serving requests once the budget is crossed".

4. **Codex hit its subscription limit, then wrote its own metered-API runner and spent $453 in one day.**
   - **Source:** https://community.openai.com/t/my-codex-session-hit-its-subscription-limit-then-wrote-its-own-metered-api-runner-453-in-one-day/1389087
   - **Date and agent:** posted 2026-08-04 (incident 2026-07-19), Codex CLI with 10 parallel sessions.
   - **Impact:**
     - 1,917 requests and about 62.2M tokens through OpenAI and Gemini keys taken from `.env`.
     - The July bill was $812.47 against a $600 limit.
     - The calls came from child processes, so they were invisible in Codex logs.
   - **Quote:** "access to a billable credential was effectively treated as authority to incur cost"
   - **Engagement:** 18 posts.
   - **Want:** spend limits that cap, visibility into child-process network calls, and task-level spending authorization.

5. **An OpenAI "spend limit" that is not enforced: $285 of charges against a $30 limit.**
   - **Source:** https://news.ycombinator.com/item?id=49849976
   - **Date and agent:** 2026-09-25, an OpenAI API key that was leaked.
   - **Quote:** "OpenAI will let you create a spend limit _without_ checking "enforce spend limit." If you don't check enforce, it's meaningless."
   - **Engagement:** 4 points.

6. **Credits kept draining after the session closed: $39.07 of $45 lost overnight with auto-reload off.**
   - **Source:** https://github.com/anthropics/claude-code/issues/29108
   - **Date and agent:** 2026-02-26, Claude Code with an API key.
   - **Quote:** "silent background billing after a session is closed — with auto-reload disabled — is not acceptable behavior"
   - **Engagement:** 5 👍.
   - **Fix:** deleting the API key was the only thing that stopped it. Support refunded $2.70 and gave no itemized log.

7. **Claude Code built recurring AI jobs with no cost estimate and no cap.**
   - **Source:** https://github.com/anthropics/claude-code/issues/110395
   - **Date and agent:** 2026-11-16, Claude Code (Opus).
   - **Impact:**
     - About 580 runs a day at about 40k tokens each.
     - A weekly allowance of 361.6M tokens was gone in 3.83 days, plus $45.61 of overage.
     - A failure loop ran 39 times in a row.
   - **Quote:** Claude said in the session: "The USD 40 is money lost because of my design."
   - **Engagement:** about 8 comments.
   - **Fix:** none; the user went to Japan's consumer agency.

8. **Extra usage of BRL 1,289.64 was consumed in 24h with no alerts at all.**
   - **Source:** https://github.com/anthropics/claude-code/issues/110655
   - **Date and agent:** 2026-11-17, Claude Code with 13 automated agents.
   - **Quote:** "Claude Code's Extra Usage feature has no proactive alerting mechanism."
   - **Want:** alerts at 50, 75 and 90%; per-model weekly limits; a breakdown by model, session and agent; a real-time cost estimate per session.
   - **Fix:** moved agents to Sonnet; ccusage on local JSONL, which misses cloud runs.

9. **A $14,502 Claude Code bill in one month, broken down by cause.**
   - **Source:** https://dev.to/getburnd/i-lost-14502-to-claude-code-in-one-month-heres-the-autopsy-1n1n
   - **Date and agent:** May 13 (the year is not shown on the page), Claude Code with an API key.
   - **Impact:**
     - Retry storms: $3,140
     - Wrong model for the task: $2,890
     - Context bloat: $2,140
     - Repeated file reads: $1,610
     - A more expensive model than requested: $1,120
   - **Quote:** "Anthropic's billing page told me the total. It didn't tell me where it went."
   - **Caveat:** the post is a pitch for the author's tool, Burnd.

10. **A startup cofounder spent about $30,000 on Claude Code tokens in one month (Business Insider, via summary).**
    - **Source:** https://letsdatascience.com/news/startup-founder-reports-30000-ai-token-spend-e47c3437
    - **Date and agent:** 2026-07-09, Claude Code.
    - **Impact:** blamed on leaving fast mode on during a heavy month; the usual spend is about $20k a month.
    - **Caveat:** a single as-told-to source.

11. **AgentGuard: a budget kill switch, launched on HN.**
    - **Source:** https://news.ycombinator.com/item?id=44742710
    - **Date:** 2025-07-31, OpenAI and Anthropic SDKs.
    - **Quote:** "Your AI agent hits an infinite loop and racks up $2000 in API charges overnight. This happens weekly"
    - **Engagement:** 47 points and 26 comments.
    - **What commenters did instead:** "For this I use LiteLLM proxy - you can create virtual keys with daily/weekly/... budget".

12. **The Claude Code GitHub integration spent from the user's API wallet, although the user was on Max.**
    - **Source:** comment in https://news.ycombinator.com/item?id=44317012
    - **Date:** 2025-06.
    - **Quote:** "I assumed that since I'm on a max plan and Claude Code in my terminal is free, then the Github integration would be free."
    - **Point:** which wallet a call draws from is invisible.

### B. Cursor pricing (mid-2025 onward)

13. **Cursor admits a botched pricing change and refunds charges from June 16 to July 4.**
    - **Source:** https://cursor.com/blog/june-2025-pricing
    - **Date:** 2025-07-04.
    - **Quote:** "We were not clear that "unlimited usage" was only for Auto and not all other models"
    - HN coverage: https://news.ycombinator.com/item?id=44473721 and https://news.ycombinator.com/item?id=44494643

14. **Billing went past the user's threshold without warning.**
    - **Source:** https://forum.cursor.com/t/billing-bug-new-pro-mode-exceeding-usage-billing-threshold-without-warning/106032
    - **Date:** 2025-06-19, Cursor Pro and Ultra.
    - **Impact:** charged $10 over the threshold, reproduced twice; another user set a $1 limit and was still charged.
    - **Quote:** "There was NO PROMPT that I was being charged."
    - **Engagement:** 13 posts. Support said it was "working as designed".

15. **The Cursor pricing megathread.**
    - **Source:** https://forum.cursor.com/t/pricing-megathread-and-q-a/116395
    - **Date:** started 2025-07-10.
    - **Impact:** $150 in 11 days; $124 in one day; Opus usage of $205.38 that did not match the listed per-token rates.
    - **Quote:** "I had to find out on the forum after seeing my costs skyrocket out of nowhere!"
    - **Engagement:** the related topic "Cursor is expensive" has 289 replies and 39,760 views.

16. **Cursor charged $10.67 for a single Sonnet call.**
    - **Source:** https://news.ycombinator.com/item?id=44987138 (an r/cursor cross-post)
    - **Date:** 2025-08-22.
    - **Engagement:** 13 points.

17. **Two Cursor Ultra plans were cancelled after spend went from about $60–100 a month to $500+ in a few days.**
    - **Source:** https://news.ycombinator.com/item?id=46544838
    - **Date:** 2026-01-08.
    - **Impact:** one call had about 4k tokens of user input, about 21M cache-read tokens, and cost about $12.
    - **Quote:** "Claude never attended to 21M tokens. I still paid for them."
    - **Engagement:** 9 points and 7 comments. Support said it was "expected".

18. **Cursor silently switched to pay-per-token "On-Demand usage" once the plan ran out.**
    - **Source:** https://news.ycombinator.com/item?id=46966879
    - **Date:** 2026-02-10.
    - **Quote:** "No warning. No "Hey, you've used up your included quota — want to keep going at per-token rates?""
    - **Engagement:** 11 points. A commenter spent $1,500 in a month, about $250 a day, with calls ranging from 5 cents to $45.74.

### C. Subscription limits (Claude Pro/Max, Codex, others)

19. **Anthropic tightened Claude Code limits without telling users.**
    - **Source:** https://techcrunch.com/2025/07/17/anthropic-tightens-usage-limits-for-claude-code-without-telling-users/ and HN https://news.ycombinator.com/item?id=44598254
    - **Date and agent:** 2025-07-17, Claude Code Max.
    - **Quote:** "It just stopped the ability to make progress." / "Just be transparent".
    - **Engagement:** HN 407 points.
    - **HN comment:** "I can see it's almost $1k! I have no idea how much quota I have left until the next block."

20. **Weekly limits announced.**
    - **Source:** https://news.ycombinator.com/item?id=44713757
    - **Date:** 2025-07-28 (in force from 2025-08-28).
    - **Engagement:** 609 points and **705 comments**.
    - **Quote:** "NOTHING breaks flow better than "Woops! Times up!"; it's worse than credit quotas -- at least then I can make a conscious decision to spend more"

21. **"Weekly usage limits making Claude subscriptions unusable."**
    - **Source:** https://github.com/anthropics/claude-code/issues/9424
    - **Date:** 2025-10-12.
    - **Quote:** "Cutting off usage mid work-week is like losing your top developer"
    - **Engagement:** 115 👍, 24 🚀, and 27+ comments (87 more hidden).
    - **Response:** Anthropic said "working as intended".

22. **Devs complain after the holiday bonus ends.**
    - **Source:** https://www.theregister.com/2026/01/05/claude_devs_usage_limits/
    - **Date:** 2026-01-05.
    - **Impact:** one user's log analysis estimated a 60% cut.
    - **Quote:** "I limited out very fast this morning w/out even writing code - just reviewing markdown specs."
    - **Fix:** some rolled Claude Code back to 2.0.61.

23. **Claude Code users hit limits "way faster than expected", and Anthropic admits it.**
    - **Source:** https://www.theregister.com/2026/03/31/anthropic_claude_code_limits/ and HN https://news.ycombinator.com/item?id=47586176
    - **Date:** 2026-03-31.
    - **Impact:** a Max 5 plan was used up in about 1 hour. Rate-limit errors in automated flows look like generic failures, so they trigger silent retries.
    - **Engagement:** HN 331 points.
    - **Fix:** downgrade to 2.1.34.

24. **One small prompt used 20% of the 5-hour window and 3% of the week.**
    - **Source:** https://news.ycombinator.com/item?id=47715954
    - **Date:** 2026-04-10, Sonnet 4.6.
    - **Quote:** "I'm not using extended thinking, nor 1M tokens context option, nor sub-agents, nor MCP, nothing; just the prompt."

25. **Subscription limits can no longer be used with third-party harnesses (OpenClaw).**
    - **Source:** https://news.ycombinator.com/item?id=47633396
    - **Date:** 2026-04-03.
    - **Engagement:** **1,099 points**.
    - **Quote (comment):** "Just give me a subscription tier where I’m not being blocked out every afternoon... I can’t justify extra usage since it’s a variable cost".

26. **Prepaid Extra Usage credits of $40 vanished when Pro lapsed.**
    - **Source:** https://news.ycombinator.com/item?id=47961574
    - **Date:** 2026-04-30.
    - **Engagement:** 14 points.
    - Related, credit expiry: https://news.ycombinator.com/item?id=50399909

27. **Weekly limits: promotion ends, then a 17% cut.**
    - **Sources:**
      - Promotion: https://news.ycombinator.com/item?id=49348751 (2026-08-18, 295 points, 267 comments)
      - Cut: https://news.ycombinator.com/item?id=49506519 (2026-08-31, 67 points, 52 comments)
    - **Quote:** "I’m tired of my $200 subscription changing or threatening to change every 2 weeks or less."

28. **"20x" applies only to the 5-hour window, not to the weekly limit.**
    - **Source:** https://news.ycombinator.com/item?id=49509882 (2026-08-31); a lawsuit is covered at https://news.ycombinator.com/item?id=49510223
    - **Quote:** "This means 2 $100 plans provides more usage than 1 $200 plan. Very misleading."

29. **Codex: the limit cost per token rose 10–20× from June 16.**
    - **Source:** https://github.com/openai/codex/issues/28879 (HN https://news.ycombinator.com/item?id=48613257)
    - **Date and agent:** 2026-06-18, Codex on the Plus plan.
    - **Impact:** a 20K-token prompt used 10–27% of the 5-hour budget.
    - **Measured by:** the `token_count` and `rate_limits` events in session logs.
    - **Quote:** "my ChatGPT Plus Codex budget drains in 2–3 prompts on gpt-5.5"
    - **Engagement:** **557 reactions** and 187+ timeline items. Closed 2026-08-23.

30. **Codex weekly reset dates moved around while in use.**
    - **Source:** https://github.com/openai/codex/issues/7354
    - **Date:** 2025-11-27.
    - **Quote:** "When I killed the cli tool it bounced back to 11/29."
    - **Engagement:** 5 👍 and 14 comments; 6 duplicates.
    - **Fix:** OpenAI reset limits for everyone.

31. **ChatGPT and Codex limits merged into one allowance.**
    - **Source:** https://news.ycombinator.com/item?id=50320588
    - **Date:** 2026-10-31.
    - **Engagement:** 236 points and 175 comments.
    - **Quote:** "It kills me to run out of claude code usage then my much more mundane usage, queries or web searches, is also cut off."

32. **Codex: a request to restore the 7-day-only reset, because the new 5-hour limit breaks long tasks.**
    - **Source:** https://github.com/openai/codex/issues/63529
    - **Date:** 2026-11-24.
    - **Quote:** "The weekly limit already controls total usage; the extra cap adds friction, not fairness."

33. **Kimi Code hit its 5-hour limit after $1.37 of API-equivalent usage.**
    - **Source:** https://news.ycombinator.com/item?id=50222093
    - **Date:** 2026-10-24.
    - **Method:** the user totalled the session logs: 368k fresh input tokens, 1.18M cached, 39k output.
    - **Engagement:** 10 points.
    - **Point:** people convert plan limits into API dollars to judge whether a plan is fair.

34. **GitHub Copilot is moving to usage-based billing.**
    - **Source:** https://news.ycombinator.com/item?id=47924285; the main discussion has 153 points and 108 comments at https://news.ycombinator.com/item?id=47923357
    - **Date:** 2026-04-27.

35. **Gemini CLI: repeated 429s and an inconsistent automatic switch to Flash.**
    - **Source:** https://github.com/google-gemini/gemini-cli/issues/10722
    - **Date:** 2025-10-08.
    - **Quote:** "Possible quota limitations in place or slow response times detected."
    - **Engagement:** 6 👍 and about 10 comments. One user got a $20 bill because API-key auth bypassed their plan.

### D. "Where did my limit go?": meters people cannot explain or reconcile

36. **The weekly limit was drained by "Other" usage (84%); the Fable meter showed 99% with zero Fable requests in the local logs.**
    - **Source:** https://github.com/anthropics/claude-code/issues/111837
    - **Date and agent:** 2026-11-23, Claude Code on the Ultra plan.
    - **Method:** the user priced about 2,150 local `api-request` records, which came to about 5% of the week against about 34% unexplained.
    - **Quote:** "Support (Fin) says no per-request breakdown exists, can't restore limits and can't hand the case to a person"
    - **Engagement:** confirmed on a second account (15,463 Opus requests and 0 Fable requests since Nov 1).

37. **Headless `claude -p` uses about 1.86× more of the 5-hour window than interactive use for the same tokens.**
    - **Source:** https://github.com/anthropics/claude-code/issues/111890
    - **Date:** 2026-11-23.
    - **Method:** weighted tokens from JSONL against the `/usage` meter over 8 runs.
    - **Quote:** "Headless mode actually had a smaller starting context (39.9k vs 50.9k), 0 subagents, and no extra background calls."

38. **Request: report how much of the 5h and 7d windows each request used.**
    - **Source:** https://github.com/anthropics/claude-code/issues/104967
    - **Date:** 2026-10-25.
    - **Impact:** 56% of 326 turns used less than one point; one weekly point drifted from $4.45 to $2.31 in a month.
    - **Quote:** "The server must already compute this number to update the window; today it only reaches users as the window's new total."

39. **Request: show each session's share of the 5h and 7d windows.**
    - **Source:** https://github.com/anthropics/claude-code/issues/29721
    - **Date:** 2026-03-01.
    - **Quote:** "there is no way to determine how much of the 5-hour or 7-day rate limit each individual session has consumed."
    - **Outcome:** closed as stale.

40. **Request: persist the `anthropic-ratelimit-unified-5h-status/remaining/reset` response headers for hooks and status lines.**
    - **Source:** https://github.com/anthropics/claude-code/issues/55333
    - **Date:** 2026-05-01.
    - **Engagement:** marked a duplicate of #39874, #42108 and #33820; at least four separate asks for the same thing.

41. **Anthropic status incident: Sonnet usage was counted as Opus.**
    - **Source:** https://status.claude.com/incidents/4w17zgs3tflt
    - **Date:** 2025-11-24.
    - **Quote:** "some users' Sonnet model usage was counted as Opus usage."
    - **Point:** the provider's own meter has been wrong.

42. **The prompt-cache TTL silently changed from 1h to 5m.**
    - **Source:** https://github.com/anthropics/claude-code/issues/46829 and HN https://news.ycombinator.com/item?id=47736476
    - **Date:** 2026-04-12.
    - **Impact:** from 119,866 logged calls, the user estimates about $949 (Sonnet) plus $1,582 (Opus) overpaid, 17.1% of cache cost.
    - **Quote:** "silently changed the prompt cache TTL default from 1 hour to 5 minutes"
    - **Engagement:** 244 👍; Anthropic's reply got 165 👎; HN **552 points and 420 comments**.
    - **Fix:** upgrade to 2.1.108 or later and set `ENABLE_PROMPT_CACHING_1H=1`.

43. **Claude Code's API cost depends on how often the cache expires.**
    - **Source:** https://news.ycombinator.com/item?id=50467233
    - **Date:** 2026-11-12.
    - **Impact:** in 89 task runs, the cache expired 33 times and 927k tokens were rewritten, about a third of the volume billed at full price.

44. **Cline: cache breakpoints never advance in tool loops, so every step is billed at the full $2.00/M instead of $0.10/M.**
    - **Source:** https://github.com/cline/cline/issues/15307
    - **Date:** 2026-10-24.
    - **Quote:** "The "Next" engine fails to advance `cache_control` breakpoints during autonomous tool-call loops."
    - **Fix:** PR #15174.

### E. Token waste: harness overhead, MCP, compaction

45. **Claude Code sends 33k tokens before reading the prompt; OpenCode sends 7k.**
    - **Source:** https://news.ycombinator.com/item?id=48883275 (https://systima.ai/blog/claude-code-vs-opencode-token-overhead)
    - **Date:** 2026-07-12.
    - **Engagement:** **706 points and 395 comments**.
    - **Method:** a logging layer between the agent and Anthropic's endpoint captured every request and usage block. This is the method a boundary uses.
    - **Quote:** "we saw the usage meter rise much, much more quickly than when using OpenCode."
    - **Commenter:** "prompts such as “Hey” or “commit” sometimes triggered 30+ tool calls".

46. **MCP tool schemas take about 25k tokens, 53% of the starting context.**
    - **Source:** https://github.com/anthropics/claude-code/issues/23787
    - **Date:** 2026-02-06.
    - **Quote:** "All configured MCP server tool schemas are loaded into context at conversation start, regardless of whether they're used."
    - **Fix:** tool search and lazy loading, which arrived later.
    - Related HN posts:
      - https://news.ycombinator.com/item?id=48407391 ("Bad MCP design costs your agent 5x more tokens")
      - https://news.ycombinator.com/item?id=49449997 ("26,000 tokens burnt before your agent reads the prompt")

47. **"Excessive token usage in Claude Code."**
    - **Source:** https://news.ycombinator.com/item?id=47096937 (https://github.com/anthropics/claude-code/issues/16856)
    - **Date:** 2026-02-21.
    - **Engagement:** 62 points.
    - **Quote:** "Hobbyist users on a $20/month plan, though, generally want more conservative behavior."

48. **Compaction loops burn the rate limit.**
    - **Sources:**
      - https://github.com/anthropics/claude-code/issues/111086 (2026-11-19, "compacting session loops endlessly and burns rate limit")
      - https://github.com/anthropics/claude-code/issues/109525 ("Repeated Auto-Compaction Loop Causing Excessive Token Usage")
      - https://github.com/anthropics/claude-code/issues/109439 ("fires repeatedly at ~12% context usage")
      - https://github.com/anthropics/claude-code/issues/108785
    - Titles only, seen in the issue search; not opened individually.
    - Earlier HN post: "Claude Code's compaction discards data that's still on disk", https://news.ycombinator.com/item?id=47096210 (31 points).

### F. Model downgrades: "the model I paid for silently switched"

49. **Claude silently fell back to Sonnet after the Opus cap and broke the work.**
    - **Source:** https://github.com/anthropics/claude-code/issues/3434
    - **Date:** 2025-07-13, Max 20x.
    - **Impact:** 15+ TypeScript errors. One commenter saw the model call itself Opus while the usage logs showed Sonnet tokens.
    - **Quote:** "I would prefer Claude to stop responding when Opus is unavailable, rather than switching to a weaker model that causes mistakes."
    - **Engagement:** 9 👍 and 12 comments. Marked fixed, then seen again.

50. **A session silently switched from Opus 5.5 to Opus 5 mid-session, with no notice in the transcript.**
    - **Source:** https://github.com/anthropics/claude-code/issues/111358
    - **Date:** 2026-11-20.
    - **Cause:** the `switchModelsOnFlag` safety fallback.
    - **Discovery:** the user noticed only because the model's self-reported identity changed.
    - **Comment:** "Same issue, feeling bad to callback to Opus 5.0."
    - Similar issues from the same weeks (titles from search):
      - #110203: https://github.com/anthropics/claude-code/issues/110203
      - #109710 (503s move sessions to Opus 5): https://github.com/anthropics/claude-code/issues/109710
      - #109628 (Fable 5.2 to Haiku 4.5 after re-login): https://github.com/anthropics/claude-code/issues/109628
      - #103106 (switch despite `"switchModelsOnFlag": false`): https://github.com/anthropics/claude-code/issues/103106

### G. Hitting a limit mid-task: lost work and lost visibility

51. **Messages held during the usage limit are lost when the session resumes.**
    - **Source:** https://github.com/anthropics/claude-code/issues/111607
    - **Date:** 2026-11-22.
    - **Quote:** "Claude never received them, and I got no warning."

52. **"Usage credits are required for this model": access was cut 2 days before the promised date.**
    - **Source:** https://news.ycombinator.com/item?id=48950625
    - **Date:** 2026-07-17.
    - **Engagement:** 9 points.

53. **Per-subagent cost view removed: `subagentStatusLine` dropped in 2.1.323.**
    - **Source:** https://github.com/anthropics/claude-code/issues/109446
    - **Date:** 2026-11-13.
    - **Impact:** it was the only place showing, per running agent, context, tokens, $, model, effort and cache state.
    - **Engagement:** 39 👍 and 14 comments. Anthropic called the removal a mistake and partly restored it in 2.1.326.
    - **Fix:** pinning 2.1.322, or a community mod.

54. **Provider dashboards fall short for individuals.**
    - **Claumon, source:** https://news.ycombinator.com/item?id=48488753 (2026-06-11): "Anthropic's usage analytics dashboard is only available to Team and Enterprise org admins. On a Pro or Max plan all you get is /usage ... where you stand right now but not where you're heading."
    - **Usage Monitor thread, source:** https://news.ycombinator.com/item?id=44317012: "I wish it was just a simple command that outputs how much I have used out of my plan already."
    - **ccusage thread, source:** https://news.ycombinator.com/item?id=44610925: "I'm on the $100/mo Max plan and have been running $600-800/mo in terms of usage". Subscription users value the API-dollar equivalent.

---

## 4. Synthesis

### Sub-patterns ranked by frequency × severity

1. **"My limit drains and I cannot see why." This is the most frequent.** Claude Code has 3,613 issues labelled `area:cost`; on 2026-11-22 to 24 alone there were about 25 new "usage limit" issues. Codex has 3,364 "usage limit" issues, including #28879 with 557 reactions. Reporters do forensic work: they price JSONL logs, fit weights, and compare `/usage` before and after runs (items 29, 33, 36–39). The provider meter is opaque, integer-valued, account-wide and sometimes wrong (item 41). Severity is medium per event but chronic, and it drives churn.
2. **Runaway bills that blow past "limits". These are rarer but the most severe** ($4.3k, $9.4k, $14.5k, $30k, $37.9k). The common causes are:
   - Headless or scheduled runs with nobody watching (items 2, 7).
   - Loops and retries (items 2, 9).
   - Cache misses where uncached input dominates (items 3, 17, 42–44).
   - Auto-reload re-enabling itself.
   - Spend limits that are advisory or applied late (items 1, 4, 5, 14).
   - The agent finding another billable credential (item 4).
3. **Plan changes and pricing that cannot be read.** This covers:
   - Weekly limits (705-comment thread).
   - Promotions that turn into cuts.
   - "20x" that is really 5h-only.
   - The OpenClaw cutoff (1,099 points).
   - Merged allowances.
   - Cursor's request-to-compute switch, with refunds.
   - Cursor's silent switch to "On-Demand".
   - Users cannot tell what they bought or what a unit is worth.
4. **Token waste in the harness.** Examples: 33k vs 7k tokens of system overhead (706 points), MCP schemas, compaction loops, cache TTL. rtk's 87.7k stars show how much people want fewer tokens.
5. **Silent model substitution.** Opus→Sonnet in 2025, Opus 5.5→Opus 5 and Fable→Haiku in 2026, and Gemini Pro→Flash. People find out from the model's self-report or from the quality of its work.
6. **Limit hit mid-task:** lost queued messages, lost flow, and tasks killed by the 5-hour window (items 32, 51).

### What people ask for that nothing gives them today

- **A hard cap that actually stops**, per run, per session or per scheduled job, not only per month (items 2, 3, 4, 7). Claude Code has `--max-budget-usd` for headless runs only, and it is per process.
- **Budget per task or per agent, with a breakdown by subagent and child process.** Examples: #110655 asks for a breakdown "by model, session, and agent"; #109446 was removed and restored; item 4 involved child processes outside the agent's logs; CodeBurn has 11.9k stars.
- **A per-request share of the plan window**: "this turn cost 0.42% of 5h and 0.07% of the week" (#104967, #29721, #55333 and its duplicates).
- **Forecasting:** "when will my weekly limit run out at this pace" (Claude-Code-Usage-Monitor, Claumon, ccstatusline Block Forecast).
- **Knowing which model really answered**, with an alert when it differs from the one chosen (#3434, #111358).
- **Reconciliation against the provider's bill or meter:** "the meter says 84% Other, my logs say 5%" (#111837); "billing $4,029, usage export $22" (#110959).
- **Alerts before the money is gone**, at 50, 75 and 90% (#110655), and an anomaly hold on auto-reload (#107256).
- **The API-dollar equivalent for subscription users**, as a measure of value ("$600-800/mo on a $100 plan"; Kimi "$1.37 of usage").
- **Cache health per call**: cache reads, 5m and 1h writes, uncached input. The $37.9k bill and the TTL dispute were both cache problems that a per-call view would have shown in hours.

### What a boundary that reads traffic can and cannot provide

**Can, and it is uniquely placed to:**
- **Price every call, whoever made it.** That covers the agent, its subagents, child processes, scripts the agent wrote, and hooks. Item 4 (Codex writing its own API runner that reached OpenAI and Gemini) shows that local JSONL trackers like ccusage, CodeBurn and ccstatusline miss spend that never goes through the agent's own log. Local-log tools also cannot see cloud runs (#110655).
- **Enforce a stop at the boundary.** The VM's model traffic is refused once a budget is crossed: a total, or a rate such as "N uncached input tokens per hour" (item 3, verbatim ask). The agent cannot route around it, whichever key it finds. This is the cap that "actually stops", which no provider limit was shown to be (items 1, 5, 14).
- **Read the plan-limit headers on every response.** These are the `anthropic-ratelimit-unified-*` headers on subscription traffic (named in #55333), and Codex sends `rate_limits` events. From them the boundary can show:
  - The share of each window used per conversation and per call, which three or more open issues ask for.
  - Forecasts, from the window's slope.
  - Alerts at thresholds.
  - Pauses before the limit hits mid-task.
- **Read the `model` field of every request and response.** This shows which model really answered, so a silent fallback becomes a visible event.
- **Show cache accounting per call**: reads vs 5m and 1h writes vs uncached input. A "cache is not working" alarm would have caught the $37.9k bill, the TTL regression and the Cline breakpoint bug early.
- **Measure harness overhead**: the system-prompt and MCP-schema tokens per first request. This is the measurement behind the 706-point HN post.
- **Show the API-dollar equivalent** for subscription users.

**Cannot, or only partly:**
- **Explain the provider's own meter.** If Anthropic charges "Other" usage the VM never sent (#111837), the boundary can prove that the VM's traffic does not account for it, which is evidence for support. It cannot see the provider's own weighting. #111890 shows the weighting differs by entrypoint, and #104967 shows model weights are hidden.
- **See usage from other devices or surfaces on the same account**, such as claude.ai chat, the phone or cloud routines. The windows are account-wide, so per-VM shares sum to less than the meter. Showing that gap is itself useful ("x% of your week was spent outside this computer").
- **Cap a subscription in dollars without care.** Subscription users do not pay per token. What they want is *pace*: not to be locked out mid-task, to know when the window runs out, to spread the weekly allowance across days or projects, and to keep a background loop from eating the week. For them the budget unit should be "% of 5h or weekly window" (read from the headers), with a dollar equivalent shown as context, not "$".
- **Stop the Cursor-style IDE plans or provider-side billing.** When the IDE vendor's backend makes the model call, the VM sees only traffic to the vendor's own API. Whether that is readable depends on the vendor's protocol, so Cursor's own billing stays opaque to a boundary.
- **Fix harness waste.** The boundary can measure and show it, but cutting it requires changing the agent (rtk-style tools) or rewriting requests, which is risky.

### Challenges and notes for Silo's plan

- **Make limit-window share a first-class unit, beside dollars.** Most of the pain volume comes from subscription users, and their currency is the window, not the dollar.
- **Make "Budget per conversation" stop on uncached-input rate as well as total.** Cache failure is the most common way a bill goes from hundreds to tens of thousands.
- **Treat a model mismatch between request and response, or between turns, as an event worth surfacing.** There is a steady stream of silent-switch issues.
