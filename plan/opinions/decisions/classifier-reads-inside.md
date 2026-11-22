# A classifier that reads inside

## The choice

An agent's part, 22 November 2026. In the fork try-bun, an agent made three commits in /work/app and
never pushed them, yet the Git state in the fork says they were pushed to GitHub. You are about to
drop the fork, and Silo lists what is only here.

The options:

- Check it from outside (recommended). Silo reads the Git state inside, but says the commits would
  come back by a pull only when it saw them leave for GitHub. Here it did not, so /work/app is on
  the list. It costs a longer list when Silo cannot tell, until you answer it. How Silo sees a push
  from outside is still to find out.
- Trust what is inside. /work/app comes back by a pull and is not on the list. It costs work you can
  lose once the fork leaves the bin, since an agent or a broken repository can make unsaved work
  look saved.
- Never read inside. Silo uses only what it saw from outside. It costs a list with every checkout on
  it, pushed or not.

Recommended: check it from outside. A mistake about what is only here is meant to fall on the safe
side, and what is inside is the agent's to change.

## Notes

What an extension reads is in the part `extensions`: `extension-tools`.
