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
- "Read" is the word for the first kind, not "see". The owner asked for better words than "show"
  and "protect" if there are any.
- No tool is settled now and no example of an interface is written: "Trying to have idiomatic
  examples here can only have us write poor API decisions that we should just think of later."

## Two kinds, and no new word

The list is organised like a token's scopes: the target first, then what the extension may do to
it.

| Target | What the extension may do | Kind | Worst case |
|---|---|---|---|
| `/work` on main | read | reads | what it reads can leave by the network rules of the extension's own computer |
| the record of main | read | reads | the same |
| the network requests of main | deny, hold | protective | the user loses time |
| main | pause | protective | the user loses time |
| the app | alert, question, panel | protective | it wastes the user's attention |

"Protective" is the word in force for an action that grants nothing and destroys nothing. An
alert already is one. A question and a panel are filed under it here. So "show" and "protect" are
not needed as words.

The record is as exact as a path, because Silo writes it itself from outside.

## What it replaces

`reading-is-a-grant` said that what a classifier may read is a grant, which rules set in advance
can give. Reading is now a tool the manifest names, granted when the extension is installed.

An earlier answer of this decision said that no call acts, so that "may use: everything" was
safe. The owner was not convinced: a call that only reads can still leak.

## Open

Forwarding alerts and asks to a phone: `forwarding-tool`. Whether an extension with nothing to
say still says it is done: `extension-says-done`.
