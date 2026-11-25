# Of what goes to an allowed host, where does the record keep the content and where only the size

## The choice

An agent's part, 25 November 2026, from the challenge of #355 (page 7).

The case: PocketOS. An agent deleted a production database with one call to Railway's API. With sizes
only, the record says "233 B sent to backboard.railway.app at 14:06, with the secret railway-token".
The restore list cannot say it was a POST, and a receipt cannot tell a push from a fetch: both are
POSTs of similar size to github.com. The boundary already reads the method of these requests, since
`secret-limits` decides on it.

The options:

- Keep the method, the host and the path of each request Silo reads, the length of its query
  string, and for a push the branches it moved; of the rest, sizes (recommended). It costs what a
  path can hold: `/users/jane@example.com` puts an address in the record. The query string, which
  more often holds a token or personal data, stays out.
- Sizes only, as the earlier answer says. It costs every case above.
- Keep the whole request. It costs the record holding bodies Silo has no reason to keep.

Recommended: method, host, path, and the branches of a push. The earlier notes do not weigh this
case. The property `record-shows-what-was-sent` changes with the answer.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

An agent's part, 22 November 2026. An agent pushes a private repository to GitHub with your token,
and you later want to know what left. Silo opens that connection to put the token in, and the
promise in force says that the record shows what was sent for the traffic Silo opens.

The options:

- Model traffic only (recommended). The record keeps what was sent to model providers and only the
  sizes for everything else. The promise is reworded to say so. It costs seeing what that push held,
  since you see only where it went and how big it was.
- Wherever a secret goes. The record also keeps the content of every request a secret was put into.
  It shows what left, and it costs a second copy of every push and upload on your disk.

Recommended: model traffic only. It is how the notes of the promise explain it, and the record does
not fill with copies of what was pushed.

The audit's finding is in the notes of `what-was-sent-to-a-host`, whose promise this decision sets
again.
