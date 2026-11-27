# A receipt on a pull request

## The choice

An agent's part, 24 November 2026. An agent spent two hours in main and opened a pull request of
40 files. The reviewer sees the diff. The reviewer does not see what the agent was asked, which
commands ran, which tests passed or failed on the way, which hosts it reached, or what it cost.

What teams do today: some make their agents put a link to the agent's session in the pull request,
because "reviewers of a pull request opened by a coding agent can't see the agent session that
produced it" (macro-inc/macro#9234). Others forbid it, because in a public repository "a
pull-request body becomes the squash commit message", and a session must never reach the public
history (HPAC-Safety/safety-report#397). Auditors ask the same question from the other side: who
authorised the change and what tested it (SOC 2, criterion CC8.1). Tools such as git-ai and
Cursor's Agent Trace record which lines an agent wrote, in Git notes or in JSON files.

The options:

- Receipts printed for the pull request, at a level the member picks (recommended). `silo` prints
  the receipts of the runs whose commits are in the pull request as Markdown. The summary level
  leaves out what was asked word for word and the conversation, for a public repository. The full
  level holds the whole receipt, for a private one. Line by line attribution in Agent Trace or Git
  notes is written by an extension, since Silo writes no export format of its own
  (`record-queried-with-ordinary-tools`). It costs a step the member takes, or asks the agent to
  take.
- A link to the receipt. It costs a place that serves receipts to the reviewer, so a server, and a
  link that leads nowhere for anyone outside the member's Mac.
- Line attribution only. It costs everything a receipt adds to "who wrote this line": the commands,
  the checks, the hosts and the spend.

Recommended: receipts printed at a level the member picks. A reviewer reads them where they already
review, and nothing is published that the member did not choose.

Which commits a run made: a commit made in a computer changes files there, which Silo sees from
outside; joining the commit to the run that made it is as sure as `the-chain` makes it.
