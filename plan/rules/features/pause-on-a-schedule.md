# Computers paused on a schedule

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
