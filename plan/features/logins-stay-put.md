## Decided

- 2026-11-10, owner: agent logins (Claude, Codex sign-ins) need special handling so that a login does not end up in forks or shared computers. How is decided later.
- Interim rule, the orchestrator's judgement, which the owner said to use: forks keep the parent's login, exports leave known login files out.

## Proposed, not confirmed by the owner

### For the owner to settle

- May Silo itself handle a subscription login (swap it in like a secret, or give each computer its own short-lived token), or does only the vendor's agent program touch it?
  - Recommendation: Only the vendor's program, with the login as a named exception to "use but not read", because a provider can change or enforce its terms at any time.
- Does a fork get its parent's agent logins by default, or only those named when it is made?
  - Recommendation: By default, with a way to leave one out, because a fork without a login cannot run its agent and the same agent already controls both.

### For whoever builds it

- Detail: The vendor login is stored outside the disk a fork copies, so it reaches a fork or an export only when Silo puts it there.
- Rule: Silo never keeps a provider's sign-in in a hidden browser window.
- Rule: A subscription login (Claude, Codex) is supported as fully as an API key.
- Test: Fork a computer whose agent command line is signed in, then let both refresh their tokens; parent and fork keep working and neither login is revoked.
- Risk: One sign-in used in main and all its forks, or shared on a team machine, can break a provider's rules on sharing a subscription.

## From the old code

The old code had it only as an idea and how much can be reused is not known.

Lessons:
- A fork keeps its parent's login because the same agent already controls both.

Where it lived, at the tag legacy-2026-11-16:
- `docs/DIRECTION.md#L174`
- `docs/direction/evidence/T14.txt`
