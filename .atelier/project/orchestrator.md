## Silo
- Owner log: one or two lines per judgement call, guess or skipped review, in the build's OWNER_REVIEW.md. Never block on it.
- Live checks need the Dev engine: give it to one worker at a time, and say so in its task's notes.
- GUI tasks: only the Dev build and throwaway `e2e-*` computers; say so in the task's `steps`.
- The `live` agent (a lastdrop name that `atelier agents` does not list) runs a live check and reads its logs; give it the check, the host and the Dev engine it may use. A GUI step goes to `gui`.
