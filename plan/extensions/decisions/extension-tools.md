# What an extension may use

## The owner's words (20 and 21 November 2026)

- An extension needs a list of authorizations, well organised, like the scopes of a token on
  GitHub. Importing it shows precisely which ones it needs.
- Agreed as worded then: "A plugin can use only the tools it declares, and installing it grants
  them."
- Putting a question to the user and showing a panel are tools, not outputs. An extension can
  wait for the answer to its question.
- What an extension shows in the app uses components the app already has, such as questions and
  alerts. Silo offers a tool that creates a panel.
- A tool is not a rule: "rules are actually kind of different and sit at a different layer."
- "Read" is the word for the first kind, not "see".
- An alert, a question and a panel do not read as protective to the owner. Denying, holding and
  pausing clearly are. The first three are "more informative".
- Where an extension can pause, the user selects the computers it can pause: all of them, or
  specific ones.
- No tool is settled now and no example of an interface is written: "Trying to have idiomatic
  examples here can only have us write poor API decisions that we should just think of later."

## Three kinds

The list is organised like a token's scopes: the target first, then what the extension may do to
it.

| A tool | Target | What the extension may do | Worst case |
|---|---|---|---|
| reads | a path on a computer; the record of a computer | read | what it reads can leave by the network rules of the extension's own computer |
| shows | the app | alert, question, panel | it wastes the user's attention |
| protects | the requests of a computer; a computer | deny, hold; pause | the user loses time |

- "Shows" and not "informs": a question asks, and a panel can hold buttons. All three put
  something in the app.
- In the kinds of action, an alert is filed as protective, since it grants nothing and destroys
  nothing (`kinds-of-action` in the part `rules`). That stays true. The kinds here say what a tool
  costs the user, which is another question.
- Choosing the computers at install is how a GitHub app is installed: on all repositories, or on
  selected ones.
- The record is as exact as a path, because Silo writes it itself from outside.

## What it replaces

`reading-is-a-grant` said that what a classifier may read is a grant, which rules set in advance
can give. Reading is now a tool the manifest names, granted when the extension is installed.

An earlier answer of this decision said that no call acts, so that "may use: everything" was
safe. The owner was not convinced: a call that only reads can still leak.

## Open

Forwarding alerts and asks to a phone: `forwarding-tool`. Whether an extension with nothing to
say still says it is done: `extension-says-done`.
