# Whether there is a rule on the age of packages

## The choice

An agent's part, 22 November 2026. A popular npm package is replaced by a version that steals keys,
and the registry removes it two hours later. During those two hours an agent in main runs `npm
install`.

The options:

- A rule you can set (recommended). You set an age, such as seven days, and Silo refuses any younger
  package from the registries the rule names. It costs a fix published today, which waits until it
  is old enough unless you allow it by name. It also costs Silo reading the traffic to those
  registries, which can break a package manager that does not accept Silo's certificate.
- Left to the package managers. pnpm, npm, Yarn and uv each have this setting. It costs the point of
  the rule: an agent with full access changes the setting or fetches the package by hand.

Recommended: a rule you can set. Most bad versions are caught within days, and a rule of this kind
only holds when it is enforced where the agent cannot change it.

## Notes

An agent's part. This decision is whether the rule exists. The design record marks that part
"Notes": polarzero did not agree to it. Where the rule is enforced is `package-age-at-the-boundary`,
which polarzero agreed to and which waits for this one.

Until 21 November 2026 both decisions had the same answer, the sentence on where the rule is
enforced. This one now answers its own question, in the words of the notes of `what-a-rule-may-do`:
"a package younger than a set age is denied". polarzero let the agent reword it and has not ruled
on it.

## Where each registry says a version's age (24 November 2026)

An agent's part, 24 November 2026. Ten package managers installed a few packages each through a
recorder in front of the real registries, which logged what they asked and what came back. The
scripts are in the project's files, under `research/2026-11-24-registries/`.

| Manager | Asks for | Carries each version's age |
| --- | --- | --- |
| npm 10.9.9 | the whole package document (`application/json`) | yes, `time` |
| pnpm 10.28, yarn 1.22, bun 1.4.4 | the short form (`application/vnd.npm.install-v1+json`) | no: only `modified`, for the whole package |
| pip 26.2 and 24.0, uv | the JSON index of PEP 691 | yes, `upload-time` on each file |
| cargo (sparse index) | one line per version | yes, `pubtime`, on all 38 versions of `itoa` and all 316 of `serde` |
| go | `@v/<version>.info` from the module proxy | a `Time`, but it is the time of the commit, which the author sets |
| maven | the `.pom` and `.jar` files | no; the files' `Last-Modified` matched the release dates |
| gem | the compact index, `/info/<gem>` | no; the `.gem` file's `Last-Modified` was a year after the release |

What it means for the rule:

- For npm, pip, uv and cargo, the age is in the answer passing through, so the boundary can read
  it there.
- For pnpm, yarn and bun, the same registry gives no age, because they ask for the short form. The
  whole document is 2 to 3 times larger (`express`: 824 KB against 343 KB). The boundary has to ask
  the registry for it separately, or change what the program asked for, which
  `what-silo-changes-in-traffic` does not allow.
- For Go, the time from the proxy is not the publish time. A version published today can carry a
  commit from years ago, so a rule that trusts it can be fooled at no cost. The time the proxy first
  saw a version is in the feed of `index.golang.org`, which can only be read in order, not asked
  per module. How Silo reads a trustworthy Go age is still to find out.
- For RubyGems, the age is in `/api/v1/versions/<gem>.json` (`created_at`), which `gem` does not
  ask for. For Maven Central, it is in the files' `Last-Modified`, or in Central's search service.
- So the boundary asks each registry for the age itself, from one source per registry, and keeps
  the answer: a version's publish time does not change. Reading it from the answer passing through
  only saves that request where it can.

Several package managers have a setting of their own for this, such as `uv --exclude-newer` and
`npm --before`. Those settings live inside the computer, where an agent can change them. The rule
at the boundary is the one an agent cannot turn off.
