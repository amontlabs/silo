# What Silo changes in model traffic

## The choice

An agent's part, 23 November 2026. Silo holds main's Anthropic API key as a secret, and an
extension that protects main denies a tool call.

The options:

- One change only (recommended). Silo puts the real key in on the way out, and the record shows
  that it did. The denied tool call is made to fail inside main, and the reply passes unchanged
  (`denied-tool-call-mechanism`). Everything else passes as it was written. It costs anything else
  Silo might do to traffic, such as trimming a long request to save money, which would need your
  yes first.
- Read only. Silo changes nothing in model traffic. It costs the API key, which then has to sit
  inside the computer, where an agent can read it.

Recommended: one change only. It does something you asked for, and the record shows it.

## Notes

- That every change is recorded is in force: `no-silent-change-to-traffic`.
- Until 23 November 2026, a denied tool call was a second change, made to the reply and to the
  later requests. The way proposed for a denial is now the probe, which changes nothing.
- Whether a login is ever swapped in is open, with logins, in `agent-programs`.
- What the record holds of a secret is in `boundary`.

## A case the answer leaves out: Claude Code on Amazon Bedrock (24 November 2026)

An agent's part, 24 November 2026. Many companies run Claude Code through Amazon Bedrock rather
than Anthropic's API. Claude Code 2.1.331 ran that way here (`CLAUDE_CODE_USE_BEDROCK=1`), against
a stand-in for Bedrock that checks every signature with botocore and knows only the real key.

- Each model request is signed with SigV4 by the AWS key. There is no key value in it to swap:
  with a placeholder key in the computer, the stand-in refused every request.
- Through the 54-line re-signer of `secret-placeholder`, the same Claude Code, holding only the
  placeholder, worked: the region's model list, the probes of three models and the conversation's
  streamed replies all passed the signature check.

So for a Bedrock user, keeping the AWS key out of the computer takes a second change to model
traffic: the boundary replaces the request's signature, which is neither a swap of a value nor
nothing. The answer's "changes it in one way only" rules that out, and with it the key outside the
computer for every Bedrock user. What I would put in its place: "Silo changes model traffic in one
way only: it puts a secret to use, by swapping its real value in for its placeholder or by signing
the request again with it. That change is recorded." The record holds the same as for a swap.
Google's Vertex AI is the same question with a token in place of a signature (the notes of
`secret-placeholder` on service accounts).
