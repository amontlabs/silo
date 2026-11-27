# An agent program's own telemetry

## The choice

An agent's part, 27 November 2026. Claude Code reports usage to Anthropic as it runs. Main's
network is limited to the hosts you allowed. If the rules Silo proposes leave the reporting host
out, Claude Code's model calls reach Anthropic with none of the reports that usually come with
them. In April 2026 a lead of Claude Code gave, as one reason for acting against other clients
that used a subscription, that their traffic came without the usual telemetry, in pages another
agent read on 27 November 2026. A computer of Silo's would then look like one of those clients.

`what-silo-sends-home` is about what Silo itself sends. This entry is about what the vendor's
program sends.

The options:

- Leave it as the vendor ships it (recommended). The program runs unmodified
  (`agent-programs-run-unmodified`), its reports included, and you can turn them off with the
  program's own setting. It costs data about your use going to the vendor, as it does when you run
  the program on your own machine.
- Cut it by default. Nothing about your use leaves but the model calls. It costs traffic that
  looks unlike the program's ordinary traffic, on an account you pay for.
- Ask at the first run. It costs one more question before you start.

Recommended: leave it as the vendor ships it.
