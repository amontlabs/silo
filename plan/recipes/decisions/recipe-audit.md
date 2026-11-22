# The audit of recipes

## The choice

An agent's part, 22 November 2026. A new release of Claude Code comes out on a Monday. Silo's
recipes take it on Thursday, after Silo has read what it changes in the computers they make.

The options:

- Pinned, with a wait of three days (recommended). Every version in Silo's recipes is fixed, and a
  release that is poisoned and pulled within three days never reaches you. A fix for a known
  security hole skips the wait. It costs new versions three days late, and Silo's own reading of
  every update.
- The newest at each make. A computer gets the newest versions on the day it is made. It costs the
  exact rebuild, since two computers made a day apart differ, and a poisoned release reaches you the
  hour it comes out.

Recommended: pinned, with a wait. The poisoned npm releases of 2025, such as those of chalk and
debug, were found and pulled within hours, and three days leaves a margin.

## Notes

The first sentence is polarzero's, 18 November 2026. polarzero added: "A company does the same for
its own." The second sentence comes from the research on Nix of the same day
(`nix-under-recipes`).
