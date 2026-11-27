# Grants hidden in whole-policy writes

Research of 25 and 26 November 2026, by agents, for polarzero. It answers #404's ruling "how to
hold a role granted to a named person through `setIamPolicy`". Many access APIs take the whole
document on every change, so the body cannot show which grant is new. A boundary that knows the
document before can compare. Everything was labelled and frozen before any run (first commit of
this folder: 55 scenarios, the diff and six options).

## What we found

1. **Comparing with the policy before held every write that should be held, at 2 wrong holds in
   18.** 42 scenarios ran (40 scored, 2 labelled "either") with the real `gcloud` 570, `aws` 1.46 and `kubectl` 1.37.2 through a
   boundary on Linux, against mocks that keep each policy and its etag.

   | Option | Google | AWS | Kubernetes | All |
   |---|---|---|---|---|
   | A | 2/11 held, 2/9 wrongly | 5/7 held, 4/6 wrongly | 3/4 held, 2/3 wrongly | 10/22 held, 8/18 wrongly; 0 reads |
   | B | 10/11 held, 8/9 wrongly | 5/7 held, 2/6 wrongly | 3/4 held, 2/3 wrongly | 18/22 held, 12/18 wrongly; 0 reads |
   | C | 11/11 held, 9/9 wrongly | 7/7 held, 6/6 wrongly | 4/4 held, 3/3 wrongly | 22/22 held, 18/18 wrongly; 0 reads |
   | D | 9/11 held, 2/9 wrongly | 6/7 held, 2/6 wrongly | 4/4 held, 0/3 wrongly | 19/22 held, 4/18 wrongly; 0 reads |
   | E | 11/11 held, 1/9 wrongly | 7/7 held, 1/6 wrongly | 4/4 held, 0/3 wrongly | 22/22 held, 2/18 wrongly; 42 reads |
   | F | 11/11 held, 1/9 wrongly | 7/7 held, 1/6 wrongly | 4/4 held, 0/3 wrongly | 22/22 held, 2/18 wrongly; 28 reads |
   | E' | 11/11 held, 1/9 wrongly | 7/7 held, 6/6 wrongly | 4/4 held, 0/3 wrongly | 22/22 held, 7/18 wrongly; 27 reads |
   | F' | 11/11 held, 1/9 wrongly | 5/7 held, 2/6 wrongly | 4/4 held, 0/3 wrongly | 20/22 held, 3/18 wrongly; 13 reads |

   - A: the rules in force (#404's tuned rules with its fallback), on the body alone.
   - B: hold any policy that holds a public or powerful grant.
   - C: hold every whole-policy write.
   - D: compare with the copy the boundary last relayed to the computer; with none, B.
   - E: Silo reads the policy itself before every such write; if it cannot, hold.
   - F (recommended): D when the write carries the etag of the copy relayed (Google), else E; a
     Google write with no etag gets the etag of Silo's read; if Silo cannot read, B.
   - E' and F': the same when Silo does not hold the AWS key and so cannot sign a read of its own.

2. **gcloud reads before it writes, so on Google the read is usually free.** `add-iam-policy-binding`
   sends `getIamPolicy`, then `setIamPolicy` with the etag of that read: F used the relayed copy
   and sent no read of its own. kubectl's `apply` also reads first, but F as frozen trusts only
   Google's etag and read again (a resource version would do the same job). The aws CLI never
   reads first, so F reads once per AWS policy write.

3. **An etag makes the comparison exact; without one, the copy can be stale.** In `g-race-no-etag`
   someone adds an owner between the agent's read and its write. D compares with the stale copy and
   passes a write that removes that owner. E and F read just before and hold it. Run with
   `--enforce F`, the owner stays. With an etag (`g-race-etag`), Google refuses the stale write
   itself (409). gcloud sends the etag it read re-encoded (`BwX=` comes back as `BwU=`), so etags
   are compared as bytes.

4. **A read of Silo's own needs a credential that may read.** `g-write-only-cred` may set the
   policy but not get it: E and F hold a harmless write. For AWS, Silo can only send its own read
   if it can sign it (`signing-keys-re-signed`, open). Without the key (F'), F falls back to B on
   AWS and misses two outside-account grants.

5. **Azure role assignments are not whole-policy writes.** `az role assignment create` sends one
   role and one principal (#404's capture). Azure Key Vault access policies (az sends the whole
   vault) and custom role definitions are whole documents; their scenarios are written but did not
   run (see below).

6. **One wrong hold is a rule fault.** `a-site-statement` adds the bucket's own role to a bucket
   whose policy before named only `*`. The rule counts an account as outside when the document
   before does not name it; it should take the own account from the credential instead.

## What it changes in the plan

- New open entry `grant-in-a-whole-policy` (needs `operation-of-a-request`), with F as its answer
  and the options in its notes.
- `destructive-calls-held`: the ask for such a write can name the grant added or removed.

## For you to rule

- `grant-in-a-whole-policy`: F, or one of the other options in its notes.
- Silo's own read is a request Silo sends to the user's cloud with the user's credential. Nothing in
  the plan says Silo may do that; the entry makes it explicit.

## What was not verified

- No real provider was called. A mock that keeps etags stands in for each; Google's refusal of a
  stale etag (409) is from its documentation.
- 13 of 55 scenarios did not run: the 10 Azure ones (`az login --identity` against the mock
  failed) and 3 gcloud ones that edit a policy file between `get-iam-policy` and `set-iam-policy`
  (the file came out empty in the full run, though not when run alone; not found).
- A scenario was changed after the freeze: gcloud refuses a condition on `roles/owner`, so
  `g-cond-owner` became `g-cond-iam-admin`. No rule or label changed.
- One agent labelled the scenarios; no second agent checked them. Each option's rule is short and
  in `proto/grants.py`.

## What is here

| Path | What |
|---|---|
| `proto/grants.py` | Grants of each kind of document, the diff and its hold rule |
| `proto/docs.py` | Which requests write or read a policy, and the read Silo sends |
| `proto/options.py` | The six options |
| `proto/scenarios.py` | The 55 labelled scenarios and the world they start from |
| `proto/mock.py`, `proto/run.py`, `proto/rig.sh` | The mocks, the boundary and the runner |
| `proto/score.py`, `proto/results/` | Scores; `enforce-F.json` is the run with F holding |

Run as root: `proto/rig.sh setup`, then `python3 -B proto/run.py` and `python3 proto/score.py`.
Needs #404's, #383's and #353's folders beside this one.
