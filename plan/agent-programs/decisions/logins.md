# What part Silo plays in the logins of agent programs

## The choice

An agent's part, 22 November 2026. You use Claude Code with your Claude subscription, and Codex with
an OpenAI API key. You make a fresh computer to run a task.

The options:

- Silo stays out of your logins (recommended). In the fresh computer you sign in to Claude Code
  again, through Anthropic's own page, as on any new machine. Silo passes the login on with each
  request and keeps it nowhere. The API key is there already, because a rule of yours gives that
  secret to the computer. Silo says plainly that a subscription works in every computer and is not
  supported as fully as an API key. It costs a sign-in in each fresh computer where you use a
  subscription.
- Silo keeps your logins and puts them in each computer. You sign in once on your Mac, and Silo adds
  your login to the requests of every computer, as it does for an API key. It costs the vendor's
  terms: Anthropic's, as read on 18 November 2026, forbid a developer to collect, store or
  intermediate the login of a Claude subscription. One large product does it anyway, with no public
  agreement with Anthropic. Several computers that hold one login can also sign each other out.
- Promise nothing yet. Silo says nothing about subscriptions until logins are designed as a whole.
  It costs anyone with a subscription knowing whether theirs works.

Recommended: Silo stays out of your logins. It matches your words of 18 November, it fits the
vendors' terms as read, and an API key, which Silo may hold for you, already works in every computer
you choose. An account at risk costs more than signing in again.

The answer also says that Silo never removes, disables or restricts a way of signing in built into
an agent program. It is one of the two conditions of Anthropic's rule for a product that preinstalls
or runs Claude Code, kept whether or not that rule applies to Silo. Silo plans no such change, so it
costs nothing now.

## The owner's position

The owner said this, on 18 November 2026: how people set up their accounts is their own
business, and Silo has no say in it. Sharing one login between computers, or switching between
several accounts, is something people do if they want. The owner does not yet have a clear idea
of this part and does not want it designed before the whole design is understood.

The owner said this after reading what is known, on 18 November 2026: how a login gets into a
computer is not for Silo to solve. A person connects the vendor's own app the official way, over
SSH, or runs the agent program inside the computer with an API key held as a Silo secret. The
question that stays open is wider than logins: how agent apps connect to computers at all
(`reaching-a-computer`, `following-the-work`, `agents-making-computers`).

## What is in force

`login-handling` and `disguising`.

## What is known

- A fresh computer that receives a copy of a login looks like a new machine to the provider and
  gets signed out.
- Several computers that hold the same login can fight over refreshing it.
- Many people hold several accounts and switch between them by hand.
- Signing in again on every fresh machine is among what people mention most (Silo's research of
  18 November 2026).
- Two mechanisms exist. Either the vendor's program keeps the login inside the computer, or Silo
  holds it outside and swaps it in like a secret. The second makes one login work in every computer
  and makes assigning an account a rule. Whether a subscription sign-in works with substitution at
  all was not tried, since Silo holds no subscription login. If this answer is ever replaced, a live
  check answers it for a provider whose terms allow it: the computer holds a placeholder, Silo swaps
  the real login into each request to the provider, and the check passes when requests succeed and
  the login is renewed without the computer ever holding it. The renewal is where it is expected to
  fail, since the program inside writes the new login itself.
- The choice is tied to reading model traffic at the boundary (the part `traffic`): if Silo
  keeps logins, it already opens the traffic to model providers.
- Anthropic's terms, read on 18 November 2026, say that a developer may not collect, store or pass
  on the credentials of a Claude subscription, and that the sign-in must complete through
  Anthropic's own flow. An API key that its owner places in a secrets manager is allowed. So the
  second mechanism fits an API key and does not fit a subscription login. The terms of other
  providers were not read.
- Reading the traffic holds no login. Claude Code documents that a proxy which inspects traffic
  works once its certificate is trusted.
- Claude Code has an official long-lived token for a machine with no window (`claude
  setup-token`). It can only make model requests. Whether a user may keep it as a Silo secret is
  a question for Anthropic.

## Sign-ins that cross to the Mac (21 November 2026)

The owner called the sign-in whose page opens on the Mac "super important". It is the feature
`sign-in-from-a-computer`, which holds no login.

## What vendors do and say

- Anthropic's own documentation recommends running Claude Code in a virtual machine, and lists
  "cloud instances, local hypervisors, and microVMs". It names another company's product for it.
  (Read on 19 November 2026.)
- Several products, hosted and local, come with Claude Code already installed and let the user sign
  in inside. That is this design's path.
- Other products go further: at least one sandbox keeps a subscription's sign-in outside the machine
  and injects it. That shows what a vendor ships, not what the terms allow, and this design does not
  lean on it.
- Anthropic's terms forbid a developer to "collect, store, or intermediate" a subscription's
  sign-in: `anthropic-reads-intermediate`.
