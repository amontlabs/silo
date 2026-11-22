# How is the number of computers running at once limited

## The choice

An agent's part, 22 November 2026. An agent keeps making forks on a Mac with 16 GB of memory. You
decided that memory decides how many fit, and nothing says where Silo stops.

The options:

- A fixed maximum, shown (recommended). Silo has a maximum that fits the Mac and shows it in the
  app. It costs waiting for a new version of Silo to raise it, even if your own case would fit more.
- A setting you raise. You can raise the maximum yourself. It costs the assurance of tests, since
  past what was tested the Mac may slow down or Silo may misbehave.
- No maximum. Silo starts computers until memory runs out. It costs finding the limit when things
  fail.

Recommended: a fixed maximum, shown. A limit known in advance is one Silo can be tested against and
can tell you before you hit it.

This would replace `computers-per-mac` and keeps the rest of what it says.
