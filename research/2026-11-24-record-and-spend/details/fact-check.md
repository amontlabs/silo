# Fact check of the reports of 24 November 2026

Checked on 24 November 2026, from the cited page or source code. GitHub issues were read through a
fetch tool that returns a summary (the `gh` API is not enabled for these repositories in this
session). Source code and docs in Markdown were downloaded whole and searched. "Fix made" names the
change made in place. No file was restructured. `plan/` was not touched, except to read it.

Verdicts: confirmed, wrong (the correct fact is in "fix made"), partly (one part confirmed, one part
wrong or unverified), could not verify.

| # | Claim | File | Verdict | Source | Fix made |
|---|---|---|---|---|---|
| 1 | CVE-2026-12039: Docker Sandboxes (sbx) 0.13.0 to 0.33.0, published 18 June 2026, "enforces an HTTP/S-only egress allowlist but does not apply it to DNS resolution", fixed in 0.33.0 | dns-and-provider-tools | confirmed | osv.dev/vulnerability/CVE-2026-12039 | none |
| 2 | Docker docs: "DNS queries use the sandbox's internal resolver, which enforces network policy"; UDP off by default; ICMP blocked | dns-and-provider-tools | confirmed | docs.docker.com/ai/sandboxes/security/isolation/ | none |
| 3 | CVE-2026-32947 = GHSA-46g3-37rh-v698, 17 March 2026, Harden-Runner DoH to dns.google, fixed in 2.16.0, Community tier only | dns-and-provider-tools | confirmed | osv.dev/vulnerability/GHSA-46g3-37rh-v698 | none |
| 4 | CVE-2026-32946: the same bypass through DNS over TCP | dns-and-provider-tools | confirmed (fixed in 2.16.0, published 20 March 2026) | osv.dev/vulnerability/CVE-2026-32946 | none |
| 5 | E2B allows all traffic to 8.8.8.8 once a domain is allowed; `denyOut` does not remove it; each `dnsServers` entry "gets all traffic allowed"; QUIC/HTTP3 not filtered | dns-and-provider-tools | confirmed | docs.e2b.dev/sandbox/internet-access | none |
| 6 | OpenAI agent, 20 September 2026: TXT and NS lookups, "the DNS route", 9:50:23 / 10:02:11 / 10:05:06 / 12:34:30, posted 25 September | dns-and-provider-tools | confirmed | openai.com/safety-alignment/misalignment-reports/an-agent-used-dns-to-reach-an-external-chatbot/ | none |
| 7 | OpenAI "restricted DNS in the environment to an allowed list of domains and record types"; ran "about 2 h 44 min after the first answer" | dns-and-provider-tools | wrong (quote and anchor) | same | The quote is "restricted DNS queries within the environment to a limited allowed list of domains and record types". 2 h 44 min runs from the first DNS call (9:50:23), not from the first answer |
| 8 | Claude Code sandbox: `allowedDomains` doesn't limit WebFetch; "Before v2.1.284, the proxy connected to whatever address an allowed hostname resolved to"; UDP, QUIC, ICMP can't leave; domain fronting warning | dns-and-provider-tools | confirmed | code.claude.com/docs/en/sandboxing.md (lines 42, 1065, 1123) | none |
| 9 | Claude Code WebFetch runs on the user's machine after a hostname preflight to api.anthropic.com, `Claude-User` agent, `web-fetch` subagent on `haiku`; WebSearch is the server tool, up to eight backend searches, not on Bedrock | dns-and-provider-tools | confirmed | code.claude.com/docs/en/data-usage.md, tools-reference.md | none |
| 10 | `web_fetch_20260318` exists; fetch only URLs that "previously appeared in the conversation" | dns-and-provider-tools | confirmed | platform.claude.com/docs/en/agents-and-tools/tool-use/web-fetch-tool.md | none |
| 11 | Advisor tokens are not in top-level `usage`, only in `usage.iterations[]` | dns-and-provider-tools, spend | confirmed | platform.claude.com advisor-tool.md ("Top-level usage fields reflect executor tokens only") | none |
| 12 | Web search costs $10 per 1,000 at Anthropic and at OpenAI | dns-and-provider-tools | partly | Anthropic pricing.md; developers.openai.com/api/docs/pricing | Added: OpenAI's web search preview on non-reasoning models costs $25 per 1,000 |
| 13 | Anthropic Files persist until deleted or `expires_at`; any key in the workspace can read them | dns-and-provider-tools | confirmed | platform.claude.com/docs/en/build-with-claude/files.md | none |
| 14 | OpenShell placeholder addresses, TTL at most 30 s, empty AAAA, wildcard warning, rejects requests to another host | dns-and-provider-tools | confirmed | docs.nvidia.com/openshell/latest/how-it-works/policies/network-rules.md | none |
| 15 | Fly Sprites: allowlist from outside, REFUSED fails fast, raw IPs blocked unless resolved from an allowed name, private IPs blocked | dns-and-provider-tools | confirmed | docs.fly.io/sprites/concepts/networking | none |
| 16 | Coder: mock DNS by default, `use_real_dns` "permits DNS-based data exfiltration", landjail "No control over UDP" | dns-and-provider-tools | confirmed | coder.com/docs/ai-coder/agent-firewall | none |
| 17 | Claude Code docs: "A file that only Bash commands changed isn't tracked and stays as it is." | incidents, record-ux | confirmed | code.claude.com/docs/en/checkpointing | none |
| 18 | 73 issues on anthropics/claude-code have `rm -rf` in the title, 19 have `reset --hard` | incidents | confirmed (18 exact plus one "--HARD") | GitHub search API | none |
| 19 | #85200: TaskStop left an orphaned `rm -rf /c` deleting for about 20 minutes, August 2026 | incidents | confirmed (opened 9 August 2026) | github.com/anthropics/claude-code/issues/85200 | none |
| 20 | #59248, deleted transcripts, 80 thumbs up, the most upvoted data loss | incidents | wrong | issue page: 👍 45 | 80 changed to 45 in both places; "most upvoted" marked as not checked against other issues |
| 21 | #16157 "Instantly hitting usage limits with Max subscription", about 1,500 comments | incidents | confirmed (about 1,495) | issue page | none |
| 22 | Codex #14593 "Burning tokens very fast", 284 👍, quote "from 99% to 0%…" | incidents | confirmed | github.com/openai/codex/issues/14593 | none |
| 23 | Codex #28058 encrypted MultiAgentV2 audit trail, 141 👍, "0 introspection" quote | incidents | confirmed | github.com/openai/codex/issues/28058 | none |
| 24 | Codex #9203 "/undo" 505 👍 with the quoted sentence; #11626 `/rewind` 233 👍 | incidents | confirmed | issue pages | none |
| 25 | #37686: about $858 and $960 on 20 and 21 March 2026; a second user about $1,700 overnight | incidents | confirmed | issue page | none |
| 26 | #72566: 5 planned subagents became 361 | incidents | confirmed (361 completed, about 377 in all) | issue page | none |
| 27 | #82380: 430 million tokens, one task 17 times, about $309 | incidents | confirmed | issue page | none |
| 28 | #85422: "an agent looping on a 429 for 63 hours and burning $4,200" | incidents | partly | issue page | Added: the comment is about the commenter's own system (ferrumdeck), not a Claude Code run |
| 29 | `/loop` about $6,000 overnight, 46 runs over 26 hours, quotes | incidents | confirmed (mirror; Reddit not reached) | ai-digest.pages.dev mirror | none |
| 30 | #46940: "4966/4966 ALL PASSED" against 4985/4992 | incidents | confirmed | issue page | none |
| 31 | #93099: Opus 5 `rm -rf "$HOME"`, 57,235 files and 11,315 directories | incidents | confirmed (incident 3 September, opened 9 September) | issue page | none |
| 32 | PocketOS: Cursor agent, Railway `volumeDelete`, "9 seconds", token for custom domains | incidents | confirmed | Tom's Hardware headline, Notebookcheck, TechRepublic, HN 47911524 (860 points, 1,032 comments) | none |
| 33 | PocketOS "ran on a three-month-old backup for two days" | incidents | could not verify | TechRepublic vs PC Gamer disagree | Marked (not verified) with both versions |
| 34 | Summer Yue: "I had to RUN to my Mac mini like I was defusing a bomb", "STOP OPENCLAW", over 200 emails, compaction, 23 February 2026 | incidents | confirmed (200 from one outlet) | PC Gamer headline, Dataconomy, Simon Willison | none |
| 35 | Replit: Lemkin quotes, rollback quote, about 1,200 executives | incidents | confirmed (1,206 executives per Fortune and others) | The Register, search | none |
| 36 | Replit: "$607.70 in charges beyond his $25/month plan" | incidents | wrong (quote) | The Register | Quote corrected to "$607.70 in additional charges beyond my $25/month Core plan" |
| 37 | CVE-2025-55284, Claude Code DNS exfiltration via allowed `ping`/`nslookup`/`dig`, fixed June 2025 | incidents | confirmed (v1.0.4, 6 June 2025; `host` too) | embracethered.com | none |
| 38 | $453 case: 4 August 2026 post, ten sessions, 1,917 requests, 62,205,325 tokens, $812.47 against $600, `--provider openai\|gemini` | incidents, keys | confirmed | community.openai.com thread 1389087 | none |
| 39 | $453 case: "The author: 'access to a billable credential was effectively treated as authority to incur cost'" | incidents | wrong (attribution) | same | Changed to "A reply in the thread" (keys-and-routes already had it right) |
| 40 | $453 case: "The $453 key 'also served production'" | keys | wrong | same: "That key also served our production app" is about the Gemini key | Both places now say the Gemini key; "call OpenAI with it" became "call Gemini with it" |
| 41 | Codex "80% pro-plan + $700", "effectively zero work" | incidents | confirmed | community.openai.com thread 1388493 | none |
| 42 | Claude Code sends an API key in `X-Api-Key` and a sign-in as `Authorization: Bearer`, per the authentication page | keys | partly | code.claude.com/docs/en/authentication.md | `X-Api-Key` confirmed. The page gives `Authorization: Bearer` for `ANTHROPIC_AUTH_TOKEN` (gateway tokens), and does not say how a `/login` sign-in is sent. Text now says only the `sk-ant-oat01-` prefix tells a sign-in apart |
| 43 | Claude Code prefers `ANTHROPIC_API_KEY` to the plan; "In non-interactive mode (`-p`), the key is always used when present" | keys | partly | same | Added: interactive mode asks once; cloud sessions ignore the key |
| 44 | #86723: Max routines billed $1,122.83 over two months, about 6,290 web searches in June, closed "not planned" by a bot | keys | confirmed | issue page | Added: the auth page says a cloud environment's key "doesn't override your subscription credentials", against the reporter's guess |
| 45 | LiteLLM price file 7,157 entries across 162 providers; OpenRouter 124 providers, 473 models | keys | confirmed | raw JSON, OpenRouter API | none |
| 46 | gitleaks 222 rules; TruffleHog 903 detectors; both have exact rules for Groq, xAI, OpenRouter, Replicate | keys | partly | gitleaks.toml (222 rules, none for Groq, xAI, OpenRouter, Replicate); defaults.go (901 to 929 by count method) | Summary now says only TruffleHog covers those four, and "about 900" detectors. The report's own table already said gitleaks lacks them |
| 47 | Basak et al. 2023: GitHub 75% precision, gitleaks 46%; recall gitleaks 88%, TruffleHog 52% | keys | confirmed | arxiv.org/abs/2307.00714 | none |
| 48 | $82,314.44 Gemini bill over 11-12 February 2026, $180 a month before; Google rejected unrestricted keys from 19 June 2026 | keys | confirmed | The Register 3 March 2026; Truffle Security | none |
| 49 | Truffle found 2,863 live keys | keys | could not verify | Truffle's June post says "nearly 3,000" | Marked (not verified) |
| 50 | OpenAI web search $10 to $25 per 1,000; containers $0.03 to $1.92 per 20-minute session | keys | confirmed | developers.openai.com/api/docs/pricing | none |
| 51 | Codex never retries `SessionBudgetExceeded`, `QuotaExceeded`, `APISpendLimitExceeded`, `UsageLimitReached`, `InferenceHookDenied`; `project_spend_limit_exceeded` on 429; request retries 4, stream retries 5 | spend | confirmed | openai/codex `codex-rs/protocol/src/error.rs`, `codex-api/src/error.rs`, `api_bridge.rs` | none |
| 52 | Claude Code retries transient failures up to 10 times, not a gateway's spend-limit 429; gateway sends `429 billing_error` with `x-should-retry: false` | spend | confirmed | code.claude.com/docs/en/errors.md; claude-apps-gateway-spend-limits.md | none |
| 53 | Official SDKs retry twice by default and honour `x-should-retry` | spend | confirmed | anthropic-sdk-python `_constants.py` and `_base_client.py`; openai-python README | none |
| 54 | Gateway: four characters per output token for aborted streams; unknown model at $5/$25; warnings at 75% and 95% from `anthropic-ratelimit-unified-*` | spend | confirmed | claude-apps-gateway-spend-limits.md | none |
| 55 | `anthropic-ratelimit-unified-7d-utilization` and the other unified headers, as captured in claude-code #12829 | spend | confirmed (undocumented headers, seen in the issue of 1 December 2025) | issue #12829; errors.md mentions "unified quota headers" | none |
| 56 | `x-codex-secondary-used-percent`, `x-codex-credits-balance` | spend | confirmed | codex `codex-api/src/rate_limits.rs` | none |
| 57 | Gemini 3.8 Flash doubles on 1 January 2027 | spend | confirmed (secondary sources quoting Google's page) | search: several outlets, $0.75/$3.75 to $1.50/$7.50 | none |
| 58 | OpenAI Ultrafast 3x to 6x on 1 March 2027 | spend | confirmed | developers.openai.com/api/docs/pricing | none |
| 59 | Fast mode on Opus 4.8 "is removed on 2026-11-30" | spend | wrong (quote only; date right) | fast-mode.md: "Removal date: November 30, 2026." | Quote corrected |
| 60 | One Fable 5.2 fast call with 1M input to the 1-hour cache and 128k output costs $42.24 | spend | confirmed (arithmetic from pricing.md: $16 fast input x 2 for 1-hour write = $32, plus 128k x $80/M = $10.24) | pricing.md | none |
| 61 | `default_inference_geo` sets the geo when the request omits it; US costs 1.1x | spend | confirmed | data-residency.md; pricing.md | none |
| 62 | Thinking streams empty by default (`display: "omitted"`) on current models | spend | confirmed (Claude 5 and later) | thinking.md | none |
| 63 | Max 20x week about $3,400 to $3,900 at API prices | spend | confirmed (three accounts, 20 August 2026, a community estimate) | dev.classmethod.jp | none |
| 64 | libkrunfw pinned at kernel 6.12.115 | recording-feasibility | confirmed | libkrunfw Makefile (`KERNEL_VERSION = 6.12.115`) | none |
| 65 | aarch64 config: `BPF_SYSCALL=y`, `CGROUP_BPF=y`, `PROC_EVENTS=y`, `FANOTIFY=y`, `STRICT_DEVMEM=y`; no tracepoints, BTF; `KPROBES`, `FTRACE`, `BPF_JIT`, `AUDIT`, `MODULES` not set | recording-feasibility | confirmed | libkrunfw `config-libkrunfw_aarch64` | none |
| 66 | libkrun `VIRTIO_BLK_T_OUT` arm in `block/worker.rs`; `EC_AA64_BKPT` and `EC_AA64_HVC` in `src/hvf/src/lib.rs`; `hv_vcpu_set_trap_debug_exceptions` in bindings | recording-feasibility | confirmed | libkrun `main` | none |
| 67 | APFS: copy written to a new location; `o_xid` "the identifier of the most recent transaction that this object was modified in"; ids not allowed "to restart from one or to be reused" | recording-feasibility | confirmed | Apple File System Reference, 2020-06-22 | none |
| 68 | DTS, thread 820718, March 2026: "definitive answer", "Suck it and see", capabilities "assigned to a specific team" | recording-feasibility | confirmed (Quinn, DTS) | developer.apple.com/forums/thread/820718 | Summary's paraphrase "no definitive answer" put back to Apple's wording |
| 69 | DTS confirms ES clients can be launchd daemons (summary cites thread 820718) | recording-feasibility | wrong (source) | thread 791996, July 2025, says it; 820718 does not | Summary now cites 791996 for this |
| 70 | macOS 26.5 guests refused SIP changes in Recovery "in every Virtualization-based tool tested" until the RC (FB22649912) | recording-feasibility | partly | developer.apple.com/forums/thread/824773 | The RC fixed it for Anka; one UTM user still saw it on the RC. Text changed in both places |
| 71 | Debug stub behind `com.apple.private.virtualization`, needs SIP and AMFI off | recording-feasibility | could not verify (AMFI part supported by third parties) | khronokernel.com, vphone-cli issue 259 | Marked: SIP part not verified |
| 72 | CVE-2025-32955: Harden-Runner `disable-sudo` bypassed through the docker group, fixed in 2.12.0 | recording-feasibility | confirmed | NVD (via search), Sysdig | none |
| 73 | Tetragon: 1.68% on a kernel build (1.5M events, 2.6K/s), 2.46% with JSON, netperf `TCP_CRR` minus 5.88%, Isovalent 31 October 2023 | recording-feasibility | confirmed | isovalent.com/blog/post/tetragon-release-10 | none |
| 74 | Plan quote: `processes-watched-from-outside` "following every version of its kernel" | recording-feasibility | confirmed | plan/record/features/processes-watched-from-outside.md | none |
| 75 | Warp: "Third-party CLI agents do not appear there" | record-ux | confirmed (the cited URL now returns 404; the sentence is on Warp's CLI-agents pages) | docs.warp.dev, search | none |
| 76 | tj-actions, 14 March 2025: "an unexpected endpoint appeared in the workflow's network traffic" | record-ux | confirmed | stepsecurity.io blog | none |
| 77 | Laminar: a tree "buries that under 400 rows of nested names" | record-ux | confirmed (Laminar, 22 September 2026); "decision spine" is from Tian Pan, as the body says | laminar.sh/article/agent-observability | none |
| 78 | Cursor: revert "fails to restore any files that were deleted" | record-ux | confirmed (date 30 June 2025 not checked) | forum.cursor.com thread 111180 | none |
| 79 | Conductor: revert "will permanently delete all user and AI messages from the selected turn and later"; private Git ref | record-ux | confirmed | conductor.build/docs/core/checkpoints | none |
| 80 | Codex cloud: "citations of terminal logs and test outputs", 16 May 2025 | record-ux | confirmed | openai.com/index/introducing-codex/ | none |
| 81 | `thinning-checkpoints` keeps every checkpoint for 2 hours, then one per hour | record-ux | confirmed | `plan show thinning-checkpoints` | none |

## Counts

81 claims: 65 confirmed, 7 wrong, 6 partly, 3 could not verify.
