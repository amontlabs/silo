# T3 Code as a client of Silo

## For the owner

An agent's part, 22 November 2026. You work from T3 Code on your Mac. Today it connects to one
computer over SSH. With this, T3 Code lists every computer, shows which runs need you, and opens the
threads of an agent in any of them.

The options:

- Build it, later (recommended). T3 Code talks to Silo as Silo's own app does. It costs a client to
  keep in step with Silo. If the work lives in Silo's own fork of T3 Code, it also costs a chat
  window that Silo then maintains, which the plan proposes Silo does not build; done in T3 Code
  itself, with its maintainers, it does not.
- Leave it out. T3 Code connects to each computer over SSH, like any app. It costs you going to
  Silo's window to see the work across computers.

Recommended: build it later, in T3 Code itself where its maintainers accept it. You want it, and an
app that already holds your agents' threads is where a view across computers helps most.

## The owner's words (21 November 2026)

The owner said T3 Code is not a competitor and should become a first-class client of Silo, through
an integration built later:

> build some integration with T3 Code in a fork. That's for later tho

"In a fork" means a fork of T3 Code's repository, not a fork of a computer.

On sharing one subscription or account across computers:

> should be easy indeed. I think the proxy thing basically. I think that's the kind of stuff we
> should see when integrating T3 Code as first-class integrated?

## An agent's notes

- Today T3 Code connects into one computer over SSH (`you-visit-a-computer`,
  `agent-programs-reach-every-computer`). What a first-class client adds is not designed. The second
  sentence of `what` is an agent's wording.
- The owner said on the same day that the view where you work with agents' threads is the client
  app's job. Those words are in the notes of `what-needs-you`.

## A question for when this is designed

Can one subscription or account be used from every computer with no work from the user?

- The owner's idea is a proxy.
- `program-and-sign-in-untouched` proposes the opposite for a subscription: "Silo holds, stores and
  injects no subscription sign-in."
- The notes of `logins` say why. Anthropic's terms, read on 18 November 2026, forbid a developer to
  collect, store or pass on the credentials of a Claude subscription. An API key held as a secret is
  allowed.
- `login-handling`, taken, says a login must not end up in a fork or a shared computer by accident.
- Whether a subscription sign-in works with substitution at all is open:
  `subscription-substitution`.

## To weigh against other entries

- `what-silo-does-not-build`, which has no verdict, proposes that Silo "builds no chat window". An
  integration kept in Silo's own fork of T3 Code makes Silo maintain part of one.
- `mac-agent-not-the-main-path`, taken, is not touched: T3 Code on the Mac is where you talk, and
  the agents run inside computers.
