# Computers paused on a schedule

## For the owner

An agent's part, 22 November 2026. You choose main and its forks, from 23:00 to 07:00. At 23:00 Silo
pauses them, your Mac goes quiet and nothing is spent overnight. At 07:00 they resume where they
were.

The options:

- Build it (recommended). Computers rest during the hours you chose. It costs care about resuming: a
  rule that resumes at 07:00 must not resume a computer you paused yourself for another reason, so
  it resumes only what it paused. A run paused in the middle of a turn may find its model connection
  gone when it resumes.
- Leave it out. You pause and resume all computers yourself, in one step each. It costs remembering
  to do it.

Recommended: build it, in the stage "later" where it sits. It is cheap and harmless, and nothing
else waits for it.

## Notes

From the list of smaller features the owner put on the roadmap on 21 November 2026. The owner's
words on the list are in `silos-own-extensions`.

The owner's list had this in one line with the alert on a run that is stuck. An agent split them:
this one is a rule on a fact, and `stuck-run-alert` is a precaution on an opinion.

## An agent's notes

- The time is a fact, so this is a rule. A pause is protective and a resume is granting, and a rule
  may do both. A precaution could not resume.
- As the tools of an extension stand (`extension-tools`), an extension's computer can pause and
  cannot resume. So this ships as rules, which an extension may add.
- Bad paths to design: a run is in the middle of a turn when its computer is paused. The Mac is
  asleep at the hour. You resume a computer by hand during its paused hours.
