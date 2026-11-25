# Network

## The choice

An agent's part, 22 November 2026. In one afternoon, main called api.anthropic.com 1,204 times for
$18.40, pushed three times to github.com with the secret github, installed 3 packages from npm that
are less than 3 days old, and sent 2.1 MB to paste.rs, a host it had never reached.

The options:

- One line per host, across runs (recommended). paste.rs stands out as new, with its 2.1 MB and the
  run that sent it, and you can deny it from its line. It costs the view of one run, which is in
  that run's receipt and not here.
- One list per run. Each run shows the hosts it reached. It costs the view of the whole computer: a
  host reached by six runs shows six times, and a new host is hidden inside one run's list.

Recommended: one line per host. A host is what you allow or deny, so it is what this place should
list.

## Notes

The sketch shows content, not design.

```text
 api.anthropic.com  1,204 calls  $18.40  claude #1, #2  14 answered by another model ⚠
 github.com           212 calls  3 pushes to you/app    secret used: github
 registry.npmjs.org    58 calls  3 packages younger than 3 days ⚠
 paste.rs               1 call   2.1 MB sent  codex     first time seen ⚠   [deny this host]
 Denied             your Mac ×2 · cloud metadata ×4
```

## From option A's first-run transcript

Agent, 25 November 2026, from `research/2026-11-25-first-run-option-a/` (gap 10 there). With #378's network, a program in main that ignores the proxy settings makes no connection, so the record has no line for it. A build that clears its environment, a launchd job started before the settings were written, or a container build inside main fail as if offline, and the morning shows nothing. The Mac end of the dead-end card sees every frame it drops, so it can count them by kind and destination address. Proposed: the place shows that count, such as "main sent 340 packets that went nowhere, to 3 addresses", as a fact.
