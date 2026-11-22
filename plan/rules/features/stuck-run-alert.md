# An alert on a run stuck too long

## For the owner

An agent's part, 22 November 2026. A run in main has been stuck for 20 minutes, asking the same
failing command again and again. You had set 15 minutes, so Silo alerts you.

The options:

- Build it as a feature of its own. It has its own place in the app and its own setting. It costs a
  second way to do what a precaution already does, to build and to explain.
- Leave it out as a feature (recommended). "Stuck" is an opinion Silo gives on each run, and you set
  a precaution on it that alerts. It costs nothing you would notice, provided a precaution can say
  "for longer than 15 minutes", which the feature for precautions then has to include.

Recommended: leave it out as a feature. You get the same alert through a precaution, and Silo can
ship that precaution ready to switch on.

## Notes

From the list of smaller features the owner put on the roadmap on 21 November 2026. The owner's
words on the list are in `what-ships-with-silo`.

Split from the owner's line on pausing on a schedule: see `pause-on-a-schedule`.

## An agent's notes

- "Stuck" is the opinion of a classifier that ships with Silo (`how-each-run-is-going`). So this is
  a precaution, and it only alerts.
- It is `precautions-you-set` with a duration added. It may be an example of that feature and not a
  feature of its own.
- An extension that reads the record and alerts could do this.
