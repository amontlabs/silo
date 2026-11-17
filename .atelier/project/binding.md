## Silo
- Nothing is built without a feature the owner took (`"verdict": "take"` in its file).
- Never write `verdict` or `built` by hand: `plan decide` and `plan finish` do, and the owner's click on the Workshop arrives as a commit.
- `plan/decisions.json` holds what the owner settled for the whole product, and a feature's notes hold what was settled for that feature. They exist so that nobody decides the same thing twice or differently: do not decide it again, and do not report as a problem what they settle. This binds reviewers too.
- Business material (prices, customers, funding) lives in Linear and never in this repository, which is public. Linear holds nothing else: product ideas and features go in `plan/`.
- Agents are chosen by role name from `atelier agents`, never by provider model; `cheap` only when the owner asks.
- A test that fails and then passes is a fix item, not a retry.
- Being rebuilt: until `plan/` and `tools/plan` land, the plan is still `docs/product/` and its tool `tools/workshop`. Do not push `main` before the Workshop is live: a push deploys the public site.
