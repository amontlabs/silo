# The response to a request an extension read

## The choice

An agent's part, 27 November 2026. polarzero: "you can record a simulation from outside by catching
it at the boundary, and yes maybe compare what actually comes back". The transaction guard
simulated a transaction and let it go on. Later the program asks the node for the receipt. The
extension wants that receipt, to compare it with its simulation. Today only a model's reply can be
read by an extension (`classifier-asked-synchronously`), and of other traffic the record keeps
sizes (`content-kept-of-what-was-sent`).

The options:

- Handed over after it passed, and not kept (recommended). A new line in the record already wakes
  an extension (`what-wakes-an-extension`), so no new wake is added. It costs a loss when the
  extension fails or falls behind, and a trial on the recorded past
  (`extension-trial-on-the-past`) has no body to replay.
- Kept in the record for the hosts a rule reads. A failed wake loses nothing and the past can be
  replayed. It costs disk, and it puts what a computer sent to an API in the record, which
  `content-kept-of-what-was-sent` chose against.
- The response waits for the extension. It costs every program a delay on every answer, and gives
  nothing a hold on the request did not.

Recommended: handed over after it passed, and not kept. To find out: whether the trial on the past
is worth keeping the content for the hosts an installed extension reads.
