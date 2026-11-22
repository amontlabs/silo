# One queue

## The choice

An agent's part, 22 November 2026. At 16:05 main asks to reach api.stripe.com, and the request is
held for 90 seconds. At the same time the fork try-bun has written 41 GB, and claude #1 has finished
in client-a.

The options:

- One list for all computers (recommended). The Stripe ask is on top with its time left, then the
  disk alert, then the finished run. You answer the ask on its line. It costs a mixed list when many
  computers run; each line names its computer.
- One list per computer. You open main to see main's asks. It costs asks that run out unseen: an ask
  in a computer you are not looking at is refused after 90 seconds.

Recommended: one list for all computers. An ask has a time limit, so it has to reach you wherever it
comes from.

## Notes

Marked Notes in the design record.
