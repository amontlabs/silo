## Decided

- 2026-11-10: the egress policy is set by the user, with requests from the agent.

## Proposed

- Property: Nothing done inside a computer can change or invalidate its network rules.
- Rule: Main has network rules of its own, and holding every project does not let it reach everything.
- Rule: Opening or closing a computer's network is one step.
- Rule: Work inside a computer never prompts per command.
- Rule: Hosts that serve documentation to agents, and tools that report bugs to their maker, are destinations like any other, and each preset names the ones it allows.
- An editable preset limits a computer's model calls to named providers and their endpoints, and covers several vendors, routers and hosted open models.
- A fork's diff lists the network attempts that were denied in it, each with the host, the time and the process that tried.
- Test: An agent makes many short calls to several model endpoints per step; the network rules hold for every call and the usage count is exact.

## Open

- Should a preset let a computer read freely and hold every network write for approval, per computer or per destination, when a read can also carry data out?
  - Suggested: No, because a read carries data out as well as a write, so the preset would promise more than it holds and ask often.
- Should a computer be able to have a network identity of its own, such as its own outbound address, as someone running one computer per account wants?
  - Suggested: No for now, because nothing in the plan needs it and whether it fits providers' terms is unknown.
- To find out: Can a preset limit which Git hosts and organisations a computer clones from, given that an organisation is a path and not a host?
- To find out: On a Linux host, do eBPF or XDP enforce a computer's network rules better than the alternatives?

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
