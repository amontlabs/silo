# Does a fork carry the sign-in of an agent program

## For the owner

An agent's part, 22 November 2026. Claude Code is signed in on main with your subscription, and you
fork main to let an agent try something risky. A fork copies the whole disk and the sign-in is a
file on it, so that agent now holds your sign-in, and the provider may sign one of the two copies
out.

The options:

- Left out unless you say (recommended). A fork starts without the sign-in, as it starts without
  secrets. You, or a rule you set, can let it carry one. It costs you a sign-in in each fork that
  needs one, and Silo has to know where each agent program keeps its sign-in.
- Copied, and shown first. The sign-in is copied with the disk, and Silo tells you before the fork
  is made. It costs the promise you took, that a login does not end up in a fork by accident, which
  becomes "never without you seeing it".
- Left open for now. Nothing is decided until logins are designed as a whole. It costs a promise
  that is not kept until then, and the plan has to say so.

Recommended: left out unless you say. A fork is where you put the agent you trust least. It already
starts with none of your secrets, so the sign-in follows the same rule.

This answers only what a fork carries. The rest of logins stays open in `logins`, as you asked. The
audit's finding is in the notes of `login-handling`. It put the second option forward first, and
named this one as the stronger.
