# How does a rule see a grant sent inside a whole access policy

## The choice

An agent's part, 26 November 2026. An agent in main runs `gcloud projects add-iam-policy-binding
proj1 --member user:mallory@example.net --role roles/owner`. gcloud reads the project's policy, adds
the binding, and sends the whole policy back with `setIamPolicy`. The project already has an owner,
so every write to its policy holds `roles/owner`, and the body alone cannot show which binding is
new. The same holds for an AWS trust policy that adds an outside account, a bucket policy that
already has a public statement, and a Kubernetes binding to `cluster-admin`.

The options, measured on 40 scenarios run with the real `gcloud`, `aws` and `kubectl` against
mocks (`research/2026-11-25-whole-policy-writes/`):

- Read the policy before, from the copy relayed or from Silo's own read (recommended, option F).
  It held 22 of 22 writes that should be held and wrongly held 2 of 18, with 28 reads of Silo's
  own. It costs one read per policy write that does not carry a known etag, a credential that may
  read the policy, and for AWS a key Silo can sign with (`signing-keys-re-signed`).
- Silo always reads first (option E): the same decisions, with 42 reads.
- Only the copy relayed (option D): no read of Silo's own, but 19 of 22 held and 4 of 18 wrongly
  held. It misses a write sent without a read first, and a change made outside the computer
  between the read and the write (a human's new owner removed).
- Hold any policy that holds a powerful role (option B): 18 of 22 held, 12 of 18 wrongly, since
  almost every project has an owner.
- Hold only what is public in the body (the rules in force, option A): 10 of 22 held, 8 of 18
  wrongly; Google owners and outside AWS accounts pass.

Recommended: F. It is the only option that held every write that should be held while holding few
harmless ones, and its read is the same read the CLIs make themselves.

## An agent's notes

- The two wrong holds of F: a credential that may write the policy but not read it (held because
  Silo cannot read), and a bucket policy whose only principal before was public, so the rule did
  not know the bucket's own account. The second is a rule fault, not a read fault: the own account
  should come from the credential.
- With `--enforce F` the race case ends right: a human adds an owner between the agent's read and
  its write without an etag; Silo's read sees the owner the write would remove and holds it.
- Azure role assignments are not whole-policy writes: each names one role and one principal and
  is readable from the body. Azure Key Vault access policies and custom role definitions are. The
  Azure scenarios did not run (the mock's managed identity login failed), nor did three gcloud
  scenarios that edit a policy file.
- gcloud refuses a condition on a basic role, so a conditional owner cannot be sent through it.
- Silo's own read is a request Silo sends to the user's provider with the user's credential. That
  is new: say so where the ask shows it.
