# What a precaution does when its classifier answers unsure

## For the owner

An agent's part, 22 November 2026. You set a precaution that asks a classifier about each tool call
in main. An agent wants to run `curl https://get.example.sh | sh`, and the classifier answers
"unsure".

The options:

- Hold it for you (recommended). The call waits, and you get an ask that shows the call and the
  classifier's opinion. It costs your attention: a classifier that is often unsure asks you often,
  and with no answer in time the call is refused.
- Deny it. The agent is told at once that the call was denied and tries something else. It costs
  work that was fine and is refused with nobody asked.
- Let it pass. Only "not safe" does anything, and the rules decide the rest. It costs the protection
  in exactly the cases the classifier could not settle.

Recommended: hold it for you, as the default, with the other two as settings. It is what you
described, and "unsure" is the one answer where a person adds something.

A default to settle when it is built, with how it is shown and changed. The owner said this.

In the owner's example of a classifier asked about each tool call, "unsure" holds the tool call
for you (`classifier-asked-synchronously`). What happens when no answer comes in time is asked in
`extension-that-does-not-answer`.
