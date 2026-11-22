# What wakes an extension

- The owner, on 20 November 2026: "wakes on" is clearly better wording than "triggered", which is
  kept for a rule or a precaution. In a manifest the field is `on`.
- The second case is the classifier: the answer comes from a fixed list
  (`what-makes-a-classifier` in the part `opinions`).
- A request waits only where a tool that protects was granted: `request-waits-only-by-precaution`.
- That every wait ends is in the part `boundary`: `classifier-wait-limit`. Whether a request goes
  on or is denied when no answer comes in time is a default, settled when this is built.
- The owner, on 21 November 2026: it must be obvious in the app when an extension's computer can
  be woken, and under which conditions. The user should know. Hence the last sentence of the
  answer, and its property.

## Found by an audit, 22 November 2026

An agent's part. This says "If it does not answer in time, the wait ends as its manifest says", and
`precaution-on-unsure` still asks what a precaution does when its classifier does not answer. An
extension granted "deny or hold the requests of main" is silent, and its manifest says "let it go
on": what it exists to stop passes. The audit proposes to narrow `precaution-on-unsure` to "What
does a precaution do when its classifier answers unsure?". If Silo should bound what a manifest may
say (deny or hold, never let go on), that is a new decision naming this one.

The question is put to the owner in `extension-that-does-not-answer`, under "For the owner".
