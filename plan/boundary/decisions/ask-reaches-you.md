- The record marks the line "(Notes; the phone app itself comes later, Owner)".
- The phone app is the feature `phone-app` in `views`. Forwarding to a phone is the feature
  `forwarding-to-a-phone` in `extensions`.

## The choice

An agent's part, 22 November 2026. An agent tries to merge at night while you are away from the Mac.
Silo holds the merge for your answer, and refuses it as not answered when the hold passes its limit.

The options:

- Silo's own apps only (recommended). You answer in Silo on the Mac or in Silo's phone app. A chat
  app or a forwarded message shows the question and leads back to Silo. It costs answering only on
  the Mac until the phone app is built, which is planned for later.
- Chat apps answer too. A reply in a chat app or to a forwarded message counts as your answer. It is
  quicker, and it puts your yes in the hands of a service Silo does not control.
- On the Mac only. No phone answers. It costs the work held while you are away, which is refused
  when the limit passes.

Recommended: Silo's own apps only. A yes can give a computer more than it has, so it comes only
through something Silo controls. The phone app keeps you reachable.

## Raised on 21 November 2026, by an agent

polarzero's roadmap has a set of rules whose asks are "answered from the phone":
`merges-and-deploys-held`. This decision proposes that a phone "does not answer". Its notes say what
fits and what does not.

## Found by an audit, 22 November 2026

An agent's part. This says a phone shows the ask and leads back to Silo, and does not answer it.
`merges-and-deploys-held` says "You can answer from your phone". A merge is held while you are away
from the Mac: if only the Mac answers, the hold passes its limit (`hold-limit`) and the merge is
refused as not answered. The audit proposes: "An ask has to reach you where you are. Only Silo's own
apps answer it, the phone app included (`phone-app`). A chat app or a forwarded message may show you
the ask and lead back to Silo; it does not answer." The feature then stands as written.

## An answer signed by the phone app, carried by any relay (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-ask-reaches-a-phone/`. The prototype pairs the phone with the Mac once,
with keys exchanged on the Mac's screen. Each ask goes to the phone sealed and signed by the Mac,
and each answer comes back sealed and signed by the phone, bound to the ask, the version of the
list the phone showed, a nonce and an expiry. The relay in between can delay or drop messages and
nothing more: 12 tamper cases, among them a forged answer, a replayed yes and a yes to a list the
Mac never sent, were refused. So "only Silo's own apps answer" holds even when a third party
carries the message; what answers is the phone app's key, not the channel.
