# Planted values

## The choice

An agent's part, 27 November 2026. You put a file named `prod-credentials.env` in main. It holds a
value Silo made, worth nothing anywhere. No task of yours touches it. One evening that value shows
up in a request to a host you never used. Whatever the agent's own account says, that is a fact,
seen from outside, and it needs no classifier.

`secret-placeholder` already records a placeholder seen at a host it was not allowed for. This
entry says the general thing and names what follows from it: since it is a fact, a rule may read
it (`rule-reads-only-facts`), so it may deny or hold exactly, where a classifier's opinion could
only act through a precaution.

The options:

- Any value Silo planted is a fact when seen (recommended). It is the one check in the plan on
  what an agent is up to that involves no model and cannot be argued with. It costs little: the
  boundary already searches what it reads for placeholders.
- Placeholders of real secrets only. It costs the planted file, which is the cheap way to learn
  that something reads what it has no reason to read.

Recommended: any value Silo planted is a fact when seen.

It has two limits. Reads of files are not recorded (`file-reads`), so a planted file is seen only
when its content leaves the computer or enters a model call. And an agent that changes the value
before sending it is not seen.
