# What does an agent get back when a hold ends without your answer

## The choice

An agent's part, 25 November 2026, from the challenges of #350 and #355, as #364 reconciles them.

The case: at 03:10 an agent's `aws` call is held and the ask reaches your phone. boto3 gives up at
60 seconds. You answer yes at 03:14. Under the earlier answer the ask closed when the hold ended, so
your yes lets nothing pass, the agent tries again, and you are asked a second time.

The options:

- The ask outlives its hold, and a late yes lets the next exact copy through once (recommended).
  "Exact" includes a hash of the body, or the client's own request id. It costs Silo keeping the
  ask and a hash of the request until you answer, and one more sentence in what the agent reads.
- The same, with a looser match on method and path only (#350). It costs the case #355 found: a yes
  to a charge of 40.00 let a charge of 9,900.00 through, since both were a POST to the same path.
- The same, with the ask closing on its own after 30 minutes (#355). It costs the morning: an ask
  raised at night is gone before you read it, which #354 found when unanswered holds vanished.
- The ask closes with the hold, as the earlier answer says.

Recommended: the ask outlives its hold, with an exact match. It is what makes an answer from the
phone useful, and the exact match is the only one that does not grant a request you never saw.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

An agent's part, 22 November 2026. At 23:00 an agent in main pushes to a host that one of your rules
holds. You are asleep, and the hold reaches its limit.

The options:

- Refused as held and not answered (recommended). The agent learns that nobody said no, so it goes
  on with other work and can try again later. It costs you repeated asks when an agent retries the
  same request again and again.
- Refused as denied. The agent gets the same message as for a no. It costs the truth, since nobody
  decided, and an agent may give up work you would have allowed.

Recommended: refused as held and not answered. It is what happened, and it covers every way a hold
can end without you, not only the two polarzero named.

## Notes

- polarzero said: if a hold times out or is cancelled from inside the computer, what was held comes
  back saying it was not answered, so that the agent understands.
- On the sentence polarzero said "kind of agree", and left the rest for when it is built
  (`how-a-hold-ends`).
- "For any reason" is wider than polarzero's two cases.
- The record's own line, Proposed, which this sentence would replace: "The request is then denied,
  with an error the agent program can read."

## The late yes, end to end with a phone (25 November 2026)

An agent's part, 25 November 2026. A prototype ran the whole path (`research/2026-11-25-ask-reaches-a-phone/`): a request held at the
boundary, the ask sent to a phone through a relay, the phone's yes back to the Mac. The late yes
worked with every client tried. A `git push` held 60 seconds and refused in Git's own terms passed
at once when sent again after a yes at 90 seconds, with no second ask. A boto3 upload resent after
its own 60 seconds joined the open ask and passed once on the yes. Codex and Claude Code, driven by
a scripted model, did the same.

How long the ask stays open matters more on the phone than how long the hold lasts. In a model of
a person answering on a phone, an ask open 30 minutes, as #355 proposed, is answered in time for
84 to 94% of asks while the person is away and awake, and for 25% of asks over an evening and a
night. An ask open until answered, as this entry says, is answered by the next morning for 96%.
That supports this entry's "until you answer it or set it aside": the yes is bound to one exact
request and spent once.

Claude Code 2.1.332 blocks a model's `sleep 50; git push`, and asks it to use its Monitor tool
instead. A retry sent at once joins the open ask and waits there, so the late yes does not depend
on the agent waiting well.
