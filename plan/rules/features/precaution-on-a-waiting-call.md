# A precaution on a tool call that waits

An agent's doubt, from drafting this feature on 21 November 2026.

A tool call is something the model's reply asks for, and the reply comes into the computer. Whether
the tool that protects a computer's requests (`extension-tools`) also covers the tool calls in its
replies is not said, and neither is whether a tool call is a request. `classifier-asked-synchronously`
puts forward that each tool call waits for the answer.

The flaw that `no-detour` names, a rule that counts denials, likely touches
`rule-actions-on-the-computer`, which does not name it.
