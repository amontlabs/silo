# Merges and deploys held for your yes

From the list of smaller features the owner put on the roadmap on 21 November 2026. The owner's
words on the list are in `silos-own-extensions`.

## An agent's notes

- It is a named set of rules (`rule-sets`), off until you switch it on. `security-framing`, taken,
  says no destination is special: these are ordinary rules, shipped as a set.
- How Silo recognises a merge into a default branch or a deploy at the boundary is to find out. A
  merge can be a push, a call to the host's API, or a click you make yourself. A deploy can be a
  request to a host or a command, and you would have to name it.
- `ask-reaches-you`, which has no verdict, proposes that only Silo's own apps answer, the phone app
  included, and that a chat app or a forwarded message "does not answer". An answer given in Silo's own `phone-app` is given in Silo. An answer
  given through `forwarding-to-a-phone` would go against it. The owner has not ruled.
- `hold-limit`, taken, gives every hold a limit. A merge that waits for your phone can pass it, and
  is then refused as held and not answered. The agent program may also give up first
  (`program-gives-up-first`).
- An extension that adds rules could ship this.
