# Is a small formal model of access written and checked before the first engine code

## The choice

An agent's part, 25 November 2026, from the challenge of #348 to `model-of-access-first`, which polarzero took.

The case: in #348's first build almost nothing is granted: your Mac's way in over SSH, and the files
you import. The access model's tables would be shaped on those two grants, and then reshaped when
secrets, hosts and rules arrive in a later build. A model of access checked on every short history is
worth most when there are grants to check.

The options:

- Write it first, as polarzero chose, but for the grants of the first build that has secrets, hosts
  and the SSH way in, written down before its code (recommended). It costs writing down grants that
  are not built yet, which is a page, not code.
- Write it first for the grants the first build has, as the earlier answer reads. It costs the
  reshaping above. The feature is small, so the cost is low.

Recommended: written for the real grants. Under #373's option A, the first build also reads model
traffic and holds budgets, which brings those grants closer, so the page is needed soon either way.
