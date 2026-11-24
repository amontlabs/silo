# What Silo needs for small teams

Research of 24 November 2026, by an agent. It asks what a team of 5 to 50 developers that runs
agents needs from Silo, and turns the answers into entries of the plan. It holds product and
technical design only. What a team would pay and why is kept in the private research repository.

## In short

1. **Most of what a team needs does not need a shared computer.** Each member keeps their own
   main. The team shares rules, budgets, receipts and secrets through what it already owns: a
   repository, its device management, its secret store and its log tool. Silo runs no server for
   any of it, and nothing passes through Amont Labs.
2. **Spend is the need with a budget already attached, and the one Silo answers best.** Vendor caps
   cover one product each. Silo's boundary sees every model call of a computer, whatever program
   makes it and whatever key it carries. A budget per member, enforced on each Mac, is exact and
   needs no server. A pooled budget across Macs is not, and is not proposed.
3. **A shared Mac mini for a team runs against Apple's licence.** macOS Tahoe 26's licence forbids
   time-sharing "within your own organization" and lets one user use macOS at a time. A Mac that
   stays on serves one person. What several people share runs on a Linux server.
4. **The secret brokers built for agents in 2026 work the way Silo's placeholders do.** TRM Labs,
   Infisical's Agent Vault and 1Password's OpenShell keep a placeholder in the agent and swap the
   value in at a proxy. Silo can take values from those stores without changing how secrets work.
5. **Two things must be in the record from its first version**, or they cost a format change later:
   a hash chain, so that an edit or a cut shows, and the person who made each control fact.

## The entries

| Need | Entries | Stage |
|---|---|---|
| The same rules on every Mac | `team-rule-set`, `rules-a-member-cannot-switch-off`, `managed-and-shown`, feature `team-rule-sets` | Later |
| Spend per member, seen by the lead | `team-budget`, `spend-the-team-sees`, feature `team-spend` | Later |
| What agents did, for a reviewer | `receipt-on-a-pull-request`, `shared-receipt-signed`, feature `receipts-on-pull-requests` | Later |
| Evidence for an auditor or an incident review | `record-edits-show`, `person-in-a-control-fact` | The ground, beside the record's format |
| Secrets from the team's store | `secret-from-a-store`, feature `secrets-from-a-store` | Later |
| A host several people reach | `shared-host-system`, `reaching-a-host`, notes added to `teams` | Later |

Each entry has its case, its options with what each costs, and a recommendation in its notes.

## The evidence

### Spend

- The Pragmatic Engineer collected spending at 27 workplaces (20 October 2026). Twelve spend $1,000
  or more per developer a month. A company of about 200 engineers caps at $1,500, and one engineer
  spent $2,200 in September. Caps are common, and "most people find them easy to get around".
  ([newsletter](https://newsletter.pragmaticengineer.com/p/whats-the-roi-of-software-engineers), free
  part only.)
- Uber capped agentic coding tools at $1,500 per employee a month after using its 2026 AI budget in
  four months ([Fortune, 26 May 2026](https://fortune.com/2026/05/26/uber-coo-ai-spending-tokens-claude-code/)).
- Claude Team and Enterprise: owners set an organisation limit, a limit per member and, on
  Enterprise, per group. At a member's limit, "usage credits" pause until the month ends. The check
  happens before a request and the cost after it, so a limit can be passed slightly. The pages say
  nothing of API keys
  ([support 12005970](https://support.claude.com/en/articles/12005970-manage-extra-usage-for-team-and-seat-based-enterprise-plans),
  [support 13799932](https://support.claude.com/en/articles/13799932-manage-groups-and-group-spend-limits-on-enterprise-plans)).
- Tailscale's Aperture holds quotas as buckets with a capacity and a refill rate, per Tailscale
  login, for example "$5.00/day", and answers 429 when a bucket is empty. It sees only traffic sent
  through it ([per-user limits](https://tailscale.com/docs/aperture/how-to/set-per-user-spending-limits)).

Why caps are walked around: each covers one product or one gateway. A personal account, an API key
from another organisation, or a second program is outside it. Anthropic's own page on managed
settings lists those routes: settings "are not delivered for accounts outside the managed
organization", and a Console API key belongs to another organisation.

### Shared rules

- Claude Code takes managed settings from a configuration profile, a file the user cannot edit, or
  Anthropic's servers. "Nothing in your own settings files or `--settings` overrides a managed key."
  The page calls the server-delivered kind "a client-side control, not a security boundary". If the
  fetch fails and the cache is older than 24 hours, Claude Code runs without them unless
  `forceRemoteSettingsRefresh` is set ([server-managed settings](https://code.claude.com/docs/en/server-managed-settings)).
- Codex reads `requirements.toml` from a system file, the cloud or the macOS management key
  `com.openai.codex:requirements_toml_base64`. Requirements are constraints "users can't override";
  defaults are starting values ([managed configuration](https://learn.chatgpt.com/docs/enterprise/managed-configuration)).
- Teams commit `.claude/settings.json` with allow and deny rules, and keep personal additions in a
  file Git ignores. A personal allow adds to the team's list and never removes its denies
  ([example](https://dev.to/aiworkspacebuilder/claude-code-permissions-for-teams-a-shared-settingsjson-that-is-safe-by-default-39ne)).

So teams share rules through a repository, and enforce them through device management. Silo's
rules have no order and a deny wins, so a member's own rules already cannot lift a team's deny.

### Pull requests and provenance

- macro-inc/macro#9234 (merged 2 November 2026) puts the agent's session link in each pull request
  it opens: "Reviewers of a pull request opened by a coding agent can't see the agent session that
  produced it."
- HPAC-Safety/safety-report#397 forbids it: "This repository is public, and a pull-request body
  becomes the squash commit message."
- git-ai stores line attribution in Git notes. Squash merges on GitHub need its paid product or its
  CI actions to keep it. Its prompts are stored outside Git ([docs](https://usegitai.com/docs)).
- Cursor's Agent Trace is a JSON format, version 0.1.0 of January 2026, that ties ranges of code to
  conversations ([InfoQ](https://www.infoq.com/news/2026/02/agent-trace-cursor)).
- Entire (February 2026) records the agent's context on each commit in the repository itself
  ([Entire](https://entire.io/blog/hello-entire-world)).
- The TODO Group proposes signed provenance built on in-toto, SLSA and Sigstore, that "can travel
  with the repository" and be verified offline
  ([TODO Group, 14 July 2026](https://todogroup.org/blog/2026-07-14-verifiable-provenance-ai-assisted-contributions/)).

The split between public and private repositories decides the design: the member picks how much a
receipt shows.

### Audit and incident review

These come from consultants and vendors, not from the AICPA.

- SOC 2 has no criterion of its own for agents. Changes made by agents fall under change management
  (CC8.1): who authorised it, what tested it, and whether the written policy matches what happened.
  Access (CC6.1) asks that agents cannot reach production or change secrets. Monitoring (CC7.2) asks
  for "timestamps, session identifiers, the developer who initiated the session, the operation
  performed, the target resource, and the result", and auditors "pick random entries and ask you to
  trace them back" ([Sentrely](https://sentrely.com/blog/claude-code-compliance/),
  [Crash Override](https://crashoverride.com/resources/knowledge-base/compliance/soc-2-ai-coding-tools)).
- Logs that fail: "logs the dev team can modify or delete", "missing time periods where agents were
  active", and a provider's usage logs, which show "token consumption, not actions" (Sentrely). Logs
  that show "only a tool name" or "a shared service account" leave ownership unclear
  ([Teleport](https://goteleport.com/blog/ai-agents-soc-2/)).
- Retention: one year is the usual floor quoted, three years for evidence archives. No source cites a
  rule.
- PocketOS, April 2026: a Cursor agent on a staging task found a Railway token "in an unrelated
  file", with no scope, and deleted the production volume and its backups. "The only audit trail was
  the agent's own post-hoc confession" ([Zenity](https://zenity.io/blog/ai-agent-database-deletion-pocketos)).

What Silo's record already gives an auditor: every control fact says who did it, a secret used is
named in the record, and a gap is a fact. What it lacks: a way for someone other than the Mac's
owner to tell that nothing was edited, and a way off the Mac. The first is `record-edits-show`. The
second already exists: an extension reads the record and sends it to the company's log tool
(`record-queried-with-ordinary-tools`).

### A Mac that stays on, and a shared host

- Guides of 2026 for a Mac mini running agents agree on Tailscale and SSH, a macOS user only for the
  agents, a GitHub machine user, and screen sharing kept for prompts. They hit full disks, updates
  that reset permissions, sleep, and FileVault asking for a password after each restart
  ([Kylon](https://kylon.io/blog/mac-mini-24-7-agents),
  [Little Might](https://www.littlemight.com/how-to-run-ai-agents-across-multiple-macs/),
  [AI Cost Check](https://aicostcheck.com/blog/spare-mac-ai-coding-agent-setup-2026)). Only one says
  several team members can share a Mac, "if tasks are queued".
- The licence of macOS Tahoe 26, read from Apple's PDF on 24 November 2026:
  - 2B(ii): a company may have "multiple individuals on a single shared Mac Computer".
  - 2B(iii): two more copies of macOS in virtual machines per Mac, for development, testing, macOS
    Server or personal use; the virtualised copies may not be used "in connection with service
    bureau, time-sharing, terminal sharing, relay service or other similar types of services".
  - 2I: you agree "not to use the Apple Software, or any of its functionality, in connection with
    service bureau, time-sharing, terminal sharing or other similar types of services, whether such
    services are being provided within your own organization or to third parties".
  - 2J: "only one user may use the Apple Software at a time".
- On Apple's developer forum (thread 845455, September 2026), an Apple engineer confirmed the
  Virtualization framework enforces two macOS virtual machines and no cap on Linux ones, and could
  not say how to ask Apple for a written agreement.

This is a reading of the licence, not legal advice. It is enough to keep a shared team host off a
Mac in the design until a lawyer says otherwise.

### Secrets

- TRM Labs (June 2026): a broker "in the agent's request path"; the agent "carries a placeholder —
  never the real secret"; values come from HashiCorp Vault. They extended Infisical's Agent Vault
  to read Vault ([TRM Labs](https://www.trmlabs.com/trm-tech-blog/never-give-an-ai-agent-a-credential-a-broker-and-the-process-we-trusted-to-build-one)).
- Infisical's Agent Vault: the agent's environment holds a placeholder such as `__github_token__`,
  which the proxy swaps "for the real credential wherever it appears in the outbound request"
  ([Infisical](https://infisical.com/blog/credential-brokering-for-ai-agents)).
- 1Password: service accounts read the vaults they are given; `op run` puts values in a process's
  environment, where an agent can read them. OpenShell, in beta, keeps placeholders in the sandbox
  ([1Password](https://1password.dev/get-started/secure-ai-access)). With OpenAI, credentials for
  Codex are injected "just-in-time" into the process
  ([SecurityWeek, May 2026](https://www.securityweek.com/1password-teams-with-openai-to-stop-ai-coding-agents-from-leaking-credentials/)).
- Doppler gives agents credentials with a lease, which the agent holds
  ([Doppler](https://www.doppler.com/blog/secure-ai-agents-defense-in-depth)).

So the market moved to Silo's design. What Silo adds is the store as the source of the value: the
member's own access to the vault decides whether their Silo gets a secret, and a revocation in the
store reaches every Mac at the next use.

## What this changes in decisions in force

Nothing is replaced. Three entries are touched by these findings:

- `teams` describes "a shared main, several people watching". With `shared-host-system`, that main
  runs on a Linux server. Its notes now say so.
- `word-budget` means a limit on "a computer, a conversation or a run". `team-budget` adds a budget
  on all of a member's computers, which would change the word if taken.
- `what-silo-sends-home` (proposed in #360) covers what Silo sends on its own. What a company's
  management sets up to send is neither the user's own choice nor Silo's; `managed-and-shown` says
  it is always shown.

## Not verified

- The Pragmatic Engineer figures beyond its free part.
- Agent Trace's own repository, which answered 404 on 24 November 2026; its description comes from
  InfoQ and Thoughtworks.
- Whether Claude's per-member limits cover API-key use. The pages do not say.
- Whether git-ai is now part of OpenAI, as a banner on its docs says.
- How each secret store lets a program sign in on a Mac nobody sits at, beyond 1Password's service
  accounts.
