# What an extension receives of a request

## The choice

An agent's part, 27 November 2026. polarzero wants extensions at the boundary to be "super easy"
to write, and gave one to ship with Silo: "simulate crypto transactions with guillotine/tevm and
hold outbound request for your review if you're getting rekt". A program in main sends
`eth_sendRawTransaction` to `mainnet.infura.io`. `what-wakes-an-extension` says Silo asks the
extension about a request that waits. No entry says what the extension is handed, and without the
body it cannot know which transaction is leaving.

The known practice is the validating admission webhook of Kubernetes: the server hands a program
the object that is about to be accepted, the program may refuse it and can never accept what the
server would refuse, the server keeps its own time limit, and the configuration says what happens
when no answer comes. Three things differ here. There is a third answer, hold, which goes to you.
"Let it go on" leaves the request to the rules. And the program runs in a computer of its own.

The options:

- The body when a rule of the manifest asks to read the host (recommended). The manifest shows the
  hosts before you install it, as it shows every rule. `ending-the-encrypted-connection` already
  reads "a host a rule asks to read". It costs Silo's certificate on one more host, and a program
  that pins its own certificate fails there.
- Never the body. The extension sees the host and the sizes. It costs the transaction guard, and
  every extension that needs to know what a request does.
- The body of every request. It costs ending every encrypted connection of the computer, which
  `ending-the-encrypted-connection` rules out.

Recommended: the body when a rule of the manifest asks to read the host.

## The bad path that matters

An agent's part. Reading one host does not stop the same bytes leaving by another. A signed
transaction is valid whichever host carries it. So an extension that reads a host sees every such request only on a computer whose network is
limited to the hosts you name (`network-modes`), and its manifest says so. For a transaction,
`blockchain-keys` moves the comparison to the request for a signature, which has one way out.
