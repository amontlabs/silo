## Silo
- Agents are chosen by role name from `atelier agents`, never by provider model; `cheap` only when the owner asks.
- Live checks run through `app/SiloUI/scripts/live/run <check> [--host mac|linux]`. A check runs on its own Dev engine instance when it can; one that drives the shared Dev engine or the Dev app holds `~/.local/state/silo-live/dev-engine.lock`. Only the Dev build and throwaway `e2e-*` computers, never production.
- A test that fails and then passes is a fix item, not a retry.
