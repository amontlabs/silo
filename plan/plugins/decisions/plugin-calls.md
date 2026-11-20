# What a plugin may use

## The owner's direction (20 and 21 November 2026)

- A plugin needs a list of authorizations, well organised, like the scopes of a token on GitHub.
  Importing it shows precisely which ones it needs.
- They are rules: "maybe it's actually a set of rules it requires you to approve." Installing the
  plugin switches them on (`import-then-install`).
- No call is settled now, and no example of one is written: "Trying to have idiomatic examples
  here can only have us write poor API decisions that we should just think of later." Only the
  areas are named.

An earlier answer of this decision said that no call acts, so that "may use: everything" was
safe. The owner was not convinced. It also contradicted `reading-is-a-grant`: a call that only
reads can still leak.

## An agent's notes

- Why a rule fits. An authorization lets the plugin do something it could not do before, which is
  a grant, and a grant is made by the user or by a rule. A rule is triggered by a fact, and a
  plugin's call arriving at the engine is one.
- When the rules are shown they can be grouped by what a wrong plugin would cost: it answers, it
  shows, it reads, it sends out.
- A set of rules switched on in one step is `rule-sets` in the part `rules`. What Silo shows
  before is `rule-set-shows-grants`.
- A panel, and forwarding alerts and asks to the user's phone, are calls and not kinds of plugin.
  Forwarding is open (`plugin-forwarding-call`), and so is whether a plugin with no output still
  says it is done (`plugin-says-done`).

## The owner's third answer (21 November 2026)

- The owner approved this decision. The verdict is not written: it waits for
  `plugin-declaration`, which this decision needs, and the same message doubts the word "rule" in
  its answer.
- The doubt: "rules are actually kind of different and sit at a different layer." What is sure for
  the owner: installing a plugin shows which tools it declared and is allowed to use.
- Putting a question to the user and showing a panel are tools a plugin uses while it runs, not
  outputs. A plugin can wait for the answer to its question.
- The output is a schema. How an output is used and how it shows in the app is designed later.
