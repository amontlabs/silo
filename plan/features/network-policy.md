## Decided

- 2026-11-10, owner: the egress policy is set by the user, with requests from the agent.

## From the old code

The old code had it built and driven end to end, about 3,900 lines, and its approach can be reused.

Lessons:
- A running computer keeps its previous rules until it restarts; the state says so.
- A preset that grew in a newer Silo counts as wider and asks again.
- Name lookups are blocked in closed mode too.
- An allowlist does not stop sending data to a listed host, and DNS can still carry data out.
- Presets can widen in a Silo update, so setting a grown preset asks again.
- Save a narrowing only if the policy is still the one it was judged against.
- A missing policy entry for a computer marked as having had one is damage, not an empty store; keep the mark outside the policy file.
- Warn on *. patterns over names anyone can publish under (*.github.io); refuse them over registry suffixes (*.co.uk).

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/network.rs`
- `app/SiloUI/src-tauri/engine/src/network_policy`
- `docs/direction/evidence/T8.txt`
- `docs/direction/evidence/V.txt`
- `docs/SiloUI-NETWORK-PLAN.md`
- `docs/SiloUI-CLI.md#network-policy`
- `docs/direction/evidence/M.txt`
- `docs/direction/checks/t8_network_policy_check.sh`
- `app/SiloUI/src-tauri/engine/src/network_policy/mod.rs`
- `app/SiloUI/src-tauri/engine/src/network_policy/store.rs`
- `app/SiloUI/src-tauri/engine/src/network_policy/service.rs`
- `docs/THREAT-MODEL.md#L111`

Calls: `network.policy.get`, `network.policy.set`, `silo network policy`, `silo secrets set --allow-shared-hosting`.
