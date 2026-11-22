# A secret an agent asks for

## The choice

An agent's part, 22 November 2026. An agent deploying your site finds it has no Cloudflare token. It
runs the `silo` command to ask for one by name. You get an ask that shows the name and the hosts the
secret would be sent to. You type the value in Silo, and the agent gets a placeholder.

The options:

- Build it (recommended). The work goes on, and the value never appears in a chat or in the
  computer. It costs a new thing agents can ask you: one can ask again and again, and one can
  propose hosts of its own choosing, so the ask has to show the hosts plainly and you have to read
  them.
- Leave it out. You notice the agent is stuck, open Silo and add the secret yourself. It costs
  waiting, and the risk that you paste the value into the chat because that is quicker.

Recommended: build it. You suggested it, it is a small step once secrets are held outside and asks
exist, and it removes the main reason a secret ends up in a conversation.

## The owner's words (21 November 2026)

> Stuff such as asking a secret for instance could just be a silo tool shouldn't be?

So the agent asks with a tool of the `silo` command inside the computer, and the answer comes from
you in Silo's prompt. The `silo` command so far only reads (`silo-command-reads-the-app`); this adds
asks to it.

From the list of smaller features the owner put on the roadmap on 21 November 2026. The owner's
words on the list are in `what-ships-with-silo`.

## An agent's notes

- Giving a secret is granting, and `what-an-agent-inside-may-do` says that what grants a computer
  more asks the user. `secrets-held-outside` already has the prompt and the placeholder. This entry
  adds that the agent starts it.
- A secret is swapped in only on the way to the hosts you allowed. So the ask shows the name and
  those hosts (`what-an-ask-shows`). The agent proposes them and you decide.
- Bad paths to design: you do not answer, and the agent is told so. You type a wrong value. An agent
  asks again and again (`full-access-inside`: nothing prompts per command inside).
- No tool of an extension grants (`what-an-extension-never-does`). So this is part of Silo and not
  an extension.
