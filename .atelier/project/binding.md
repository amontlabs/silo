## Silo
- Agents are chosen by role name from `atelier agents`, never by provider model; `cheap` only when the owner asks.
- Live checks run through `app/SiloUI/scripts/live/run <check> [--host mac|linux]`. A check runs on its own Dev engine instance when it can; one that drives the shared Dev engine or the Dev app holds `~/.local/state/silo-live/dev-engine.lock`. Only the Dev build and throwaway `e2e-*` computers, never production.
- A test that fails and then passes is a fix item, not a retry.
- Product: `docs/product/PRODUCT.md` says what Silo is and what comes first, `JOURNEYS.md` how it is used and what works today, `DECISIONS.md` what was decided. Read them before building. A change to what a user does or sees updates its step in `JOURNEYS.md` in the same merge.
- Business material (prices, customers, funding) never goes in this repository: it is public.
- Silo is being restarted from an empty tree. Read `docs/product/RESTART.md` before anything else: it says what is decided, where the work stands and what to do when a session starts. Add nothing to the current tree except the dashboard and `docs/product/`.
