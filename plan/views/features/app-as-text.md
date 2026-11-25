# What the app shows, as text

## The choice

An agent's part, 25 November 2026, from the challenge of #348.

The case: the first build has no app. `silo` is its only surface, and it lists what changed in main
last night. This feature needs `now-in-the-app`, so under the earlier text the command line waits for
the app's Now place to be built first.

The options:

- Drop the need on `now-in-the-app`, and let a view reach the command line first (recommended). It is
  what `silo-command-reads-the-app`, which polarzero took, already allows, and what #348's
  `silo-command` builds. It costs a view whose text format is set before the app shows it.
- Keep the need, as the earlier text says. It costs the first build its only surface.

Recommended: let a view reach the command line first.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

An agent's part, 22 November 2026. An agent in main wants to know what its last run changed before
it carries on. It uses Silo's command and reads that run's receipt.

The options:

- Build it (recommended). The agent reads, as text, the same receipt you see, limited to main and
  without the precautions set on main. It costs a text form to keep in step with every screen, and a
  format that programs come to depend on.
- Leave it out. Agents learn what happened from their own memory and the files. It costs answers
  made from guesses instead of the record.

Recommended: build it, and give each place its text when the place is built, not at the end. Your
agents then read what happened instead of guessing it.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

"Everything the app shows can be read with `silo`" may be a condition on each place, built with it, and not one feature built last.

## polarzero's words (21 November 2026)

Asked about gathering what happened in a computer to hand it to an agent:

> I'm not sure this is kinda something we're already planning, tools for agents to easily access
> such monitoring for their computer or others, based on the permissions.

## What the plan already says, from an agent

- `silo-command-reads-the-app`, taken, is this feature: an agent reads the record with the `silo`
  command.
- `what-an-agent-inside-may-do`, taken, gives agents inside a computer every tool, scoped by where
  they are.
- Two things are not said yet. Whether a computer reads its own record by default is open in
  `computer-reads-its-record`. Nothing says what lets a computer read the record of another one.
  `kinds-of-action` files letting a classifier read a record as granting, and the same would hold
  here: you or a rule grants it.
- `where-precautions-live`, taken, limits it: by default a computer sees nothing of the opinions and
  precautions about it.
