---
"silo-ui": minor
---

`silo fold <fork> --promote` now works for Linux computers. The bundled runtime can rename a stopped computer, so the fork takes main's name and published ports, main is kept stopped as `main-before-<time>`, saved SSH entries follow, and the fork's parked scheduled jobs are put back through the guest. `silo undo main` swaps them back.
