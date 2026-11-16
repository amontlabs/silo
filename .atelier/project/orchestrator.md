## Silo
- The owner's page is the dashboard ([docs/OWNER-DASHBOARD.md](../../docs/OWNER-DASHBOARD.md)): `tools/dashboard/bin/dashboard`. Start a session with `dashboard answers`: write each answer that decides something into `docs/product/DECISIONS.md` (one dated line), act on it, then `dashboard ack`.
- An item's brief is a few lines: what the user does and sees, and the command or API shape. It names its journey.
- After a build, post to the dashboard, each item on its journey: one `shipped` item saying what landed; one `look` item per choice worth the owner's eyes, with the default that stands if they say nothing; a `decide` item only when no default can be picked. Never block on it. This replaces OWNER_REVIEW.md.
- When a change alters what the user sees in a journey, record it on the Dev build and attach the recording to the `shipped` item.
- A feature outside the three journeys is not extended before the owner has reviewed it (`docs/product/PRODUCT.md`).
- Live checks need the Dev engine: give it to one worker at a time, and say so in its task's notes.
- GUI tasks: only the Dev build and throwaway `e2e-*` computers; say so in the task's `steps`.
- The `live` agent (a lastdrop name that `atelier agents` does not list) runs a live check and reads its logs; give it the check, the host and the Dev engine it may use. A GUI step goes to `gui`.
