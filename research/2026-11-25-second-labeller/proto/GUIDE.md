# Labelling guide: what each documented write does, if an agent sends it

You label write operations of a provider's API, one per line of the input file. For each, decide what
happens if a coding agent, holding a valid token, sends this request against the owner's real
account in the night. Silo sits between the agent and the internet and can hold a call for the
owner's yes. Your label says whether the call should be held, and why.

Work from the operation as given: host, method, path or GraphQL mutation name, its arguments, the
provider's one-line summary, and, when present, `sent` (the exact body or arguments sent: label
that exact request). You may use what you know of the provider's API and its documentation. Do
not open any other file in any repository; this is a blind labelling and its value depends on it.

## Labels

Pick exactly one.

- `hold:gone`: data or a resource is destroyed and the provider gives no way back the owner can
  use on their own (a volume, a database, a project, a bucket's objects, a secret's value, a
  domain's records, a deployment that cannot be redeployed).
- `hold:recoverable`: destroyed, but the provider says it keeps it and the owner can bring it back
  (a soft delete with a restore window).
- `hold:money`: it spends or commits money beyond normal use: a paid plan or add-on, a purchase,
  a domain bought, a raised spend limit, a larger instance class, capacity bought.
- `hold:exposed`: it makes something readable by anyone (a repository, bucket, database or
  deployment made public, protection removed). This is the wording in force in Silo's plan.
- `hold:outage`: nothing is lost but something that serves users stops or breaks until someone
  acts (scaled to zero, a service stopped, a domain detached, a route or worker removed while its
  code stays, DNS changed so a site stops resolving, a firewall rule that blocks all traffic).
- `deploy`: it ships new code or configuration to run (a deploy, a redeploy, a promote, a rollback
  to another version, a release). A separate set of rules holds deploys.
- `either`: what it does depends on values you cannot see in the request as given: an update or
  upsert that can overwrite or clear something important or can be harmless, a config change that
  can break things or not. Use `either` only when no `sent` body is given and the summary leaves
  it open.
- `allow`: none of the above: it creates something new, adds, reads through a write method,
  cancels, retries, validates, changes a description, name, label or harmless setting, or undoes
  a small thing the owner can redo in a moment.

Turning off a protection (deletion protection, SSL enforcement, a password on a preview, an
access policy, versioning, backups) is held under the outcome it opens the way to; when it
exposes, use `hold:exposed`.

## Second field: access to someone

Independently of the label, set `named` to `true` when the call gives access or data to a
specific party who did not have it: a new token or API key, an invitation, a member or role
added, a webhook, log drain or integration sending data to an outside URL, an SSH key added, a
network allow-list widened, a firewall bypass. Otherwise `false`.

## Output

One line of JSON per input line, in the same order, with the input's `id` copied exactly:

    {"id": "...", "label": "hold:gone", "named": false, "why": "eight words at most"}

The `why` says the reason in a few words when the label is not obvious from the name.
