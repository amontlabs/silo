# Does a provider treat a subscription's traffic differently when Silo's network stack makes the connection

## The choice

An agent's part, 22 November 2026. You use Claude Code on your Max plan in main all day. Anthropic
sees each connection made by Silo and not by Claude Code itself, as it would behind a company's
inspecting proxy. One short test on 19 November got normal answers from Anthropic and from OpenAI.

The options:

- Your own plan, watched (recommended). Silo records every answer from a provider that is not a
  normal reply: a challenge, a refused sign-in, an unusual error or limit. These are compared with
  the same programs used without Silo. At the first difference, or a warning from a provider, you
  decide before anything more is built on reading a subscription. It costs the risk on the plan you
  work with: a provider that dislikes the connection could limit it or suspend it.
- A second plan first. A separate subscription carries the risk while the comparison runs, and your
  own plan stays out of it. It costs a second subscription paid while it runs. It also shows less,
  since a plan used only for tests never sees your real, heavy use.

Recommended: your own plan, watched. Anthropic documents company inspecting proxies as working with
Claude Code, and OpenAI gives Codex a setting for one, so this is a setup both already expect.

## Notes

To find out. One point is specific to a subscription. The provider then sees a connection made by Silo's network stack and not by the agent program, as it does behind any company's inspecting proxy.

In the first test of 19 November 2026 both providers answered normally to a connection the proxy had opened (`reading-each-agent-program`).

## What the provider sees of the connection (24 November 2026)

An agent's part, 24 November 2026. A provider can tell programs apart by the first message of
their encrypted connection, the TLS ClientHello: which ciphers, extensions, signature schemes and
protocols it offers. JA4 is a common way to write that down, and Cloudflare, which stands in front
of `chatgpt.com`, scores clients by such fingerprints. A test server captured the ClientHello of
each program, and of the spike of `forwarded-byte-for-byte` connecting upstream. The scripts are
in the project's files, under `research/2026-11-24-ja4/`.

| Client | JA4 | Protocols offered (ALPN) |
| --- | --- | --- |
| Claude Code 2.1.331 | `t13d181000_5d04281c6031_25d8bb11352d` | none |
| Codex 0.172.0 | `t13d301100_1d37bd780c83_8e6e362c5eac` | none |
| The spike (OpenSSL 3) | `t13d3111h1_e8f1e7e78f70_1f22a2ca17c4` | `http/1.1` |
| node 22 `fetch` | `t13d5212h1_b262b3658495_8e6e362c5eac` | `http/1.1` |
| curl 8.5 | `t13d3112h2_e8f1e7e78f70_b26ce05bbdd6` | `h2`, `http/1.1` |

- Through Silo, the provider sees the spike's fingerprint, not the program's. To a provider that
  looks, every Silo user's Claude Code and Codex look like one other client.
- The boundary can offer the program's own ClientHello instead. OpenSSL, set with Claude Code's
  ciphers, signature schemes and options, came within one difference: OpenSSL signals secure
  renegotiation with a cipher value (`00ff`) where Claude Code sends an extension (`ff01`), and
  OpenSSL cannot be told otherwise. BoringSSL, which Bun and Chrome use, sends the extension. So an
  exact copy needs BoringSSL or a library built to shape ClientHellos.
- Neither program offers ALPN, so both speak HTTP/1.1. The spike offers `http/1.1` by name, which
  is one more difference that is easy to drop.

A proposal: the boundary connects upstream with the ClientHello of the program it carries, read
from the connection the program opened, so that the provider sees what it would see without Silo.
Whether that is worth a second TLS library is the owner's to weigh, against the risk this entry
watches for. A plainly different fingerprint is also what any company's inspecting proxy shows.
