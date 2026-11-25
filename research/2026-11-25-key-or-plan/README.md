# Claude with an API key or with a plan, per computer

Research of 25 November 2026, by an agent, for polarzero. Option A of #373 reads model traffic in
the first build without `secrets-held-outside`. If Anthropic says a proxy may not read a plan's
traffic, `anthropic-reads-intermediate` falls back to reading "only in computers whose API key it
holds as a secret", and in option A's first build Silo holds no key. This specifies what replaces
that fallback, as the open entry `claude-key-or-plan`, and builds it into #374's boundary.

| File | Holds |
|---|---|
| [`boundary.patch`](boundary.patch) | The setting in #374's boundary: `src/keyplan.zig` (new) and the two places `main.zig` calls it |
| [`progs.patch`](progs.patch), [`keyplan.json`](keyplan.json) | Five Claude Codes on five computers set three ways |
| [`run.sh`](run.sh) | Copies #374's folder, applies the patches, builds, runs, checks |
| [`check.py`](check.py) | Eight checks on the record and the stand-in's bill |
| [`results/run.txt`](results/run.txt) | The output of the run reported here |

## What was found

- **One setting per computer is enough, and it is small.** "Claude with an API key" or "with a
  plan", read when a connection to api.anthropic.com opens. About 60 lines in the boundary.
- **A computer set for a plan passes the connection on from the program's own TLS handshake.** The
  boundary peeks at the ClientHello for the host and then copies bytes both ways. Claude Code
  there trusted only the provider's authority, not Silo's, and worked, so Silo never ended the
  connection, never saw the sign-in, and Anthropic sees Claude Code's own fingerprint.
- **A plan in a computer set for a key stops at once, with the reason.** Claude Code 2.1.332 got a
  400 in Anthropic's shape and printed "Silo: this computer is set for Claude with an API key, and
  Claude Code is signed in with a plan here. ..." in 0.6 s, with no retry. Its management calls
  with the plan's token (`/api/claude_code/settings`, `/policy_limits`) were refused too.
- **The setting decides, not the request.** An API key in a computer set for a plan passes unread
  and uncounted, and Silo cannot know. The app should say so on such a computer.
- **No sign-in value reached the record** in any of the five computers.

| Computer set for | Signed in with | Calls read | Passed unread | Exit |
|---|---|---:|---:|---|
| an API key | an API key | 12 | 0 | 0 |
| an API key | a plan | 0 (5 refused) | 0 | 1, Silo's sentence |
| a plan | a plan | 0 | 14 connections | 0 |
| a plan | an API key | 0 | 14 connections | 0 |
| no setting (today) | a plan | 12 | 0 | 0 |

The stand-in provider served 48 calls; the boundary read 24.

## What it changes in the plan, by entry id

- `claude-key-or-plan`: new open decision, with its choice, the options (a setting; hold the key
  outside; read no Anthropic traffic; guess from the sign-in), what each answer from Anthropic
  changes, and the trial. Placed after `subscription-and-key-alike`.
- Notes added: `anthropic-reads-intermediate` (the fallback's gap, and the sentence that would
  replace it), `subscription-and-key-alike`, `provider-sees-silos-connection`,
  `model-traffic-read` (should need the new entry under option A), and `silos-own-model-calls`
  (linked billing, a way for Silo's own calls with no key in Silo).

No verdict, answer or list was changed.

## How to run it

On Linux, as root, with Zig 0.15.2, OpenSSL, zlib and zstd, Python 3 and Claude Code:

```
research/2026-11-25-key-or-plan/run.sh /tmp/key-or-plan
```

It works in a private network and mount namespace, as #374's harness does.

## Not verified

- Real Anthropic. The provider is #374's stand-in.
- The path through #378's proxy, where the host comes from `CONNECT` before any encryption. That is
  simpler than reading the ClientHello, which the trial did because #374's harness points
  `/etc/hosts` at the boundary.
- Changing the setting on a running computer (closing its open Anthropic connections) is specified,
  not built.
- Claude Code builds other than the npm 2.1.332; the native build main gets is untested through
  any boundary (#382, gap 11).
