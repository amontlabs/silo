# The calls of a plugin

A panel, and forwarding alerts and asks to the user's phone, become calls and not kinds of plugin.
Two gaps remain: none of the four calls forwards (`plugin-forwarding-call`), and whether a
plugin with no output still says it is done (`plugin-says-done`).

## The owner's answer (20 November 2026)

The owner is not convinced by making "everything" safe: "I think we should just have different
categories depending on dangerosity." The owner also wants to know the first calls Silo would ship
and their interface, as "a convenient categories and interface and api", and is not sure this is
to design now.

An agent's note: reading is already a grant (`reading-is-a-grant`, taken), so "may use:
everything" cannot be harmless. The last sentence of this decision's answer contradicts it.

## The owner's second answer (20 November 2026)

- The categories are what the owner meant, and no more than this: a plugin needs a list of
  authorizations, well organised, like the scopes of a token on GitHub. Installing it shows
  precisely which ones it needs.
- A hunch of the owner: "probably this should be tied to rules, and maybe it's actually a set of
  rules it requires you to approve."
- No call is settled now, and no example of one is written: "Trying to have idiomatic examples
  here can only have us write poor API decisions that we should just think of later." The owner
  wants only the areas a plugin's calls would touch.
