# What an extension may use

## The owner's words (20 and 21 November 2026)

- An extension needs a list of authorizations, well organised, like the scopes of a token on
  GitHub. Importing it shows precisely which ones it needs.
- Agreed as worded: an extension "can use only the tools it declares, and installing it grants
  them". The sentence then said plugin.
- Putting a question to the user and showing a panel are tools the back end uses while it runs,
  not outputs. A back end can wait for the answer to its question.
- A tool is not a rule: "rules are actually kind of different and sit at a different layer."
- The name of the first group is "read", the standard word.
- No tool is settled now and no example of an interface is written: "Trying to have idiomatic
  examples here can only have us write poor API decisions that we should just think of later."

## The groups

| A tool | Example | Worst case |
|---|---|---|
| reads | `/work` on main; the record of main | what it reads can leave by the network rules of the extension's own computer |
| shows | a panel; a notification; a question to the user | it wastes the user's attention |
| protects | deny or hold the network requests of main it is asked about; pause main | the user loses time |

The list is organised like a token's scopes: the target first, then what the extension may do to
it. The record is as exact as a path, because Silo writes it itself from outside.

## What it replaces

`reading-is-a-grant` said that what a classifier may read is a grant, which rules set in advance
can give. Reading is now a tool the extension declares, granted when it is installed.

An earlier answer of this decision said that no call acts, so that "may use: everything" was
safe. The owner was not convinced: a call that only reads can still leak.

## Open

Forwarding alerts and asks to a phone: `forwarding-tool`. Whether a back end with nothing to say
still says it is done: `back-end-says-done`.
