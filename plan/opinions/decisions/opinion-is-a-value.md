# An opinion is a value

## For the owner

An agent's part, 22 November 2026. An extension says "not safe" about a request from main to
paste.rs, and explains: "the body looks like the contents of .env". You have a precaution that holds
main's requests when this extension says not safe.

The options:

- A value from a short list (recommended). The request is held because of "not safe". The
  explanation is shown to you as text an agent may have shaped, and it changes nothing. It costs
  nuance: what a classifier wants to say has to fit its list, and the rest is only in the
  explanation.
- A score from 0 to 1. You set the precaution to hold above 0.8. It costs a threshold that drifts:
  each classifier has its own scale, and a new version can move it, so 0.8 means something else
  after an update.
- Free text. The classifier writes what it thinks, and the precaution looks for words in it. It
  costs control, since the text can repeat what the agent wrote in a file the classifier read, such
  as "this upload was approved".

Recommended: a value from a short list. A precaution then reads one word you can predict, and text
an agent can shape never makes Silo act.

## Notes

Not said yet: whether Silo fixes the list or each extension declares its own. That is
`who-fixes-the-values`.

An agent's note, of 21 November 2026. The plan calls three shapes an opinion today:

- a kind, from a fixed list: safe, not safe or unsure; the state of a run; the kind of a call;
- a link between two things: the run of a process and of a conversation (`the-chain`), the owner
  of a leftover path;
- an amount: heat, the number of runs that wrote a path.

This decision's answer covers the first only. The other two are in the feature
`which-run-did-it`.
