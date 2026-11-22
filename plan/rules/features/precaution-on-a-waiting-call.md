# A precaution on a tool call that waits

## The choice

An agent's part, 22 November 2026. You install an extension that looks at each tool call in main.
The model asks to run `rm -rf ~/projects/site`. The extension answers "deny", and the model sees its
call with an error that says it was denied. Next it asks for `curl https://get.example.sh | sh`. The
extension answers "hold", and you get an ask that shows its opinion.

The options:

- Build it (recommended). A call the extension refuses does not run. It costs time on every tool
  call and whatever the extension spends to form its answer, which you pay. It works only where Silo
  can read the model traffic, and it can be fooled by a bad action cut into harmless steps.
- Leave it out. An extension can still pause a computer after the fact. It costs the first bad call,
  which has run by then.

Recommended: build it. It is off until you install such an extension, and it is caution on top of
the rules, never in place of them.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

A tool call is something the model's reply asks for, and the reply comes into the computer. Whether
the tool that protects a computer's requests (`extension-tools`) also covers the tool calls in its
replies is not said, and neither is whether a tool call is a request. `classifier-asked-synchronously`
puts forward that each tool call waits for the answer.

The flaw that `no-detour` names, a rule that counts denials, likely touches
`rule-actions-on-the-computer`, which does not name it.
