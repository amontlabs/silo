# How long may a hold last

## The choice

An agent's part, 25 November 2026, from the challenges of #350, #355 and #369, as #364 reconciles them. The
earlier answer rested on Claude Code ending a command at two minutes. Claude Code 2.1.331 moves a
command to the background at 2 minutes and stops it at 12, and Codex 0.172 kept one for 20 minutes
(#347, #355). What gives up first is the tool, not the agent program.

The case: at 02:00 an agent runs `aws s3 rm` on a bucket a rule holds, and another runs `git push` to
a branch a rule holds. boto3 closes its connection at 60 seconds and sends the request again, three
times. git waits as long as it is held.

The options:

- A length per client, and model replies kept alive (recommended). The rule for a host carries a
  length below its client's timeout: under 60 seconds for `aws`, up to 10 minutes for `git push`,
  90 seconds when nothing is known. A held model reply gets its headers at once and a keep-alive
  every 5 seconds (`ping` on Anthropic's Messages, a comment on Gemini's stream, an event Codex
  ignores on OpenAI's Responses), and every program tried then waited 300 seconds or more (#369).
  It costs a length per known client, kept with the rule, and a keep-alive chosen per format, since
  an unknown event broke OpenCode and Cline.
- 10 minutes for every hold (#355). It costs the tools that give up sooner: `aws` closes at 60
  seconds, Silo's words go to a closed connection, and boto3's three resends each join the ask.
- 90 seconds for every hold, as the earlier answer says (#350). It cuts a `git push` hold short for
  no reason, and a model reply held 90 seconds when no program needed it cut.

Recommended: a length per client, with model replies kept alive. Since an ask now outlives its hold
(`hold-sentence`), a hold no longer has to last until you wake up; it only has to end before the
program stops listening, which is a number per client.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

An agent's part, 22 November 2026. At 10:00 a command of an agent in main pushes to a host that a
rule holds. You are in a meeting until 10:30. The agent program ends any command that runs for more
than two minutes.

The options:

- Short, under the program's own patience (recommended). The hold lasts 90 seconds unless the rule
  says otherwise. The agent then gets Silo's message, held and not answered, and can try again
  later. It costs you being at the Mac to say yes, and a second try by the agent when you were not.
- Long, so that you can answer when you are back. The hold lasts an hour. It costs the message: the
  program gives up after its two minutes, the agent sees a bare timeout, and your yes at 10:30 lets
  nothing pass because the request is gone.

Recommended: short, under the program's own patience. A hold only works while the program still
waits, so a long limit gives you an ask that no yes can serve. The number is a start, to be
measured.

## Notes

To find out. The record asks, with it, how each agent program takes the error at the limit.

Nothing has been measured yet.
