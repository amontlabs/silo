# What Silo changes in model traffic

## The choice

An agent's part, 25 November 2026. Four drafts each need a change to traffic that the earlier answer, "in one
way only", rules out. #364 (section 5) gathers them into one wording; this choice adds #355's fourth.

The cases:

- An allowed page quotes your key, and `secret-values` removes it from the reply: a second change,
  already in the plan (#355).
- Bedrock or App Store Connect: the request is signed with a placeholder key, and the boundary signs
  it again with the real one (#347, #361).
- A token minted from a secret comes back as a placeholder (#356, `exchanged-tokens`).
- You paste a token into a chat, and on your yes it becomes a secret; later model requests carry its
  placeholder, so the model never writes the real value into a file that gets pushed (#355, page 5;
  the prompt cache keeps every prefix, since the swap starts with the first request that carries
  the value).
- On your yes, a destructive call is sent with a final snapshot first (#353, `made-undoable`).

The options:

- Three ways only, named: put a secret to use, keep a secret out, and make a call undoable on your
  yes (recommended). Every change is recorded with what it replaced, as
  `no-silent-change-to-traffic` requires.
- The same without the third way. It leaves `made-undoable` out until #353's entries are ruled on.
- One way only, as the earlier answer says. It is already untrue once `secret-values` is taken.

Recommended: three ways, named. The question now covers the traffic Silo reads, not only model
traffic, since re-signing and undoable calls touch other hosts. `forwarded-byte-for-byte` points to
this answer and needs no change for it.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

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

Google's Vertex AI, tried the same day with Claude Code (`CLAUDE_CODE_USE_VERTEX=1`) and a service
account key file whose private key is a placeholder: the boundary signed each token request again
with the real key and handed back a placeholder in place of the access token, then swapped the
real token into each model request. The conversation worked, and neither the real key nor a real
token reached the computer. Two things showed: Claude Code ignores the key file's `token_uri` and
always asks `oauth2.googleapis.com`, so the boundary must read that host; and it asked for a new
token before each model request, five in a short run. For Vertex the change to model traffic is a
plain swap, which the answer allows; the change to the token traffic is the new kind.
