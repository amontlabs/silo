- The record marks the line "(Notes; the phone app itself comes later, Owner)".
- The phone app is the feature `phone-app` in `views`. Forwarding to a phone is the feature
  `forwarding-to-a-phone` in `extensions`.

## Raised on 21 November 2026, by an agent

The owner's roadmap has a set of rules whose asks are "answered from the phone":
`merges-and-deploys-held`. This decision proposes that a phone "does not answer". Its notes say what
fits and what does not.

## Found by an audit, 22 November 2026

An agent's part. This says a phone shows the ask and leads back to Silo, and does not answer it.
`merges-and-deploys-held` says "You can answer from your phone". A merge is held while you are away
from the Mac: if only the Mac answers, the hold passes its limit (`hold-limit`) and the merge is
refused as not answered. The audit proposes: "An ask has to reach you where you are. Only Silo's own
apps answer it, the phone app included (`phone-app`). A chat app or a forwarded message may show you
the ask and lead back to Silo; it does not answer." The feature then stands as written.
