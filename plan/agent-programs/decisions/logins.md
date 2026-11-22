# What part Silo plays in the logins of agent programs

## For the owner

An agent's part, 22 November 2026. You use Claude Code with your Claude subscription, and Codex with
an OpenAI API key. You make a fresh computer to run a task.

The options:

- Silo stays out of your logins (recommended). In the fresh computer you sign in to Claude Code
  again, the vendor's own way. The API key is there already, because a rule of yours gives that
  secret to the computer. Silo only keeps a login out of a fork unless you say so, and lets a
  sign-in page open on your Mac. It costs a sign-in in each fresh computer where you use a
  subscription.
- Silo keeps your logins and puts them in each computer. You sign in once, and every computer has
  the login. It costs the vendor's terms: Anthropic's, as read on 18 November 2026, forbid a product
  to collect, store or pass on the login of a Claude subscription. Several computers that hold one
  login can also sign each other out.

Recommended: Silo stays out of your logins. It matches your words of 18 November, it fits the
vendors' terms as read, and an API key, which Silo may hold for you, already works in every computer
you choose.

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
  holds it outside and swaps it in like a secret. The second makes one login work in every
  computer and makes assigning an account a rule. Whether a subscription sign-in works with
  substitution at all is to find out: `subscription-substitution`.
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
