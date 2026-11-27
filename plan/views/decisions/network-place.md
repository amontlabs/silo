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

An agent's part, 24 November 2026, from the prototype in `research/2026-11-24-morning-review`. Codex reached four hosts at start, before any
work: github.com, api.github.com, chatgpt.com and ab.chatgpt.com. "First time seen" alerted on
all four, and a hold on writes to api.github.com held Codex's own plugin sync, which nobody
answered. The view would want to tell a program's own traffic from what the model asked for.

## From option A's first-run transcript

Agent, 25 November 2026, from `research/2026-11-25-first-run-option-a/` (gap 10 there). With #378's network, a program in main that ignores the proxy settings makes no connection, so the record has no line for it. A build that clears its environment, a launchd job started before the settings were written, or a container build inside main fail as if offline, and the morning shows nothing. The Mac end of the dead-end card sees every frame it drops, so it can count them by kind and destination address. Proposed: the place shows that count, such as "main sent 340 packets that went nowhere, to 3 addresses", as a fact.

## The frames the dead-end card drops, counted

Agent, 25 November 2026, from `research/2026-11-25-way-in-before-the-relay/` (`cardend`). The Mac end of the dead-end card was built and counts every frame it drops, by kind and destination, with the first five ports of each. From inside main, a program that ignored the proxy tried a host on the local network, the Mac, cloud metadata, an internet host and a scan of the router. The count read, among others, "tcp connect 10.77.0.2 ports 7778", "tcp connect 169.254.169.254 ports 80", "icmp 203.0.113.10" and "tcp connect 10.254.0.1 ports 1,2,3,4,5,... x65535": a scan is one line, not 65,535. Each SYN a program sends again is counted again, so a count is of packets, not of attempts. A lookup that falls back to a public resolver shows too ("udp 8.8.8.8 ports 53 x6" in the run). A program that resolves names only through main's configured servers leaves no frame, since main has none: the count shows only what was sent to an address. The proposed line on the place stands, measured.

## The count, bounded

Agent, 25 November 2026, from `research/2026-11-25-cardend-hardened/`. Main chooses every address it sends to, so a count with one line per address grew without end under a flood of frames each to a new address: 200,000 frames left 200,000 lines, and the card slowed to a few frames a second. The count now holds at most 256 lines; past that, a new address is counted on its kind's line "other addresses" ("tcp connect, other addresses ports 443,... x150000"). The place's sentence then reads "to more than 250 addresses", which is the fact that matters.

## More research on this (27 November 2026)

An agent's part, 27 November 2026. These reports bear on this entry, and its notes did not point to them. The answer is unchanged.

- `research/2026-11-25-red-team-round-two/` (#435), finding 2: the list of dropped destinations needs a bound. The fix keeps 256 lines and counts the rest in one line of their kind.
