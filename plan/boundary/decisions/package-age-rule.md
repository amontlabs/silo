# Whether there is a rule on the age of packages

## The choice

An agent's part, 22 November 2026. A popular npm package is replaced by a version that steals keys,
and the registry removes it two hours later. During those two hours an agent in main runs `npm
install`.

The options:

- A rule you can set (recommended). You set an age, such as seven days, and Silo refuses any younger
  package from the registries the rule names. It costs a fix published today, which waits until it
  is old enough unless you allow it by name. It also costs Silo reading the traffic to those
  registries, which can break a package manager that does not accept Silo's certificate.
- Left to the package managers. pnpm, npm, Yarn and uv each have this setting. It costs the point of
  the rule: an agent with full access changes the setting or fetches the package by hand.

Recommended: a rule you can set. Most bad versions are caught within days, and a rule of this kind
only holds when it is enforced where the agent cannot change it.

## Notes

An agent's part. This decision is whether the rule exists. The design record marks that part
"Notes": polarzero did not agree to it. Where the rule is enforced is `package-age-at-the-boundary`,
which polarzero agreed to and which waits for this one.

Until 21 November 2026 both decisions had the same answer, the sentence on where the rule is
enforced. This one now answers its own question, in the words of the notes of `what-a-rule-may-do`:
"a package younger than a set age is denied". polarzero let the agent reword it and has not ruled
on it.
