# An ask that reaches a phone, end to end

Research of 25 November 2026, by agents, for polarzero. #350 found that a 90-second hold fails
when the ask reaches a phone. #355 and #364 answered with an ask that outlives its hold and a late
yes that lets the same request through once; #364's wording is now `hold-sentence` on the
integration branch (#391). Nobody had run the whole path: a request held at the boundary, the ask
sent to a phone, the person's answer coming back to the Mac, and the agent's request passing. This
prototype runs that path on Linux with real git, curl, pip, boto3, Claude Code 2.1.332 and Codex
0.172.0, and models how long a person takes to answer.

## What we found

1. **The path works with no port open on the Mac.** The Mac and the phone each make only
   outbound requests to a relay. Every message is sealed to the receiver and signed by the sender
   (X25519, ChaCha20-Poly1305 and Ed25519, keys exchanged once when the phone is paired). On this
   machine the ask reached the phone 2.0 s after the request was held: the 2 s that
   `asks-for-a-burst` waits to gather a burst, plus about 10 ms through the relay. A yes released
   the request about 20 ms after the phone sent it. Real push delivery adds about 1 s, with a tail
   of minutes on a bad connection (sources below).

2. **The relay cannot answer for you.** 12 of 12 tamper cases came out as expected: an answer
   signed by another key, bytes the relay made up, one bit changed, an old yes replayed for the
   same request, a yes to a list the Mac never sent, an answer an hour old, a yes the relay held
   past its expiry, and a dropped answer were all refused or simply lost. A yes to the version of
   the ask the phone showed let that line pass and not a line added after. The phone refused an
   ask the relay made up. The relay could log only a topic, a size and a time.
   (`results/tamper.json`)

3. **Away from the Mac, the late yes does the work, not the hold.** With the person away and
   awake (median 6 minutes to attend a notification), the model gives:

   | Hold | Answered while the request still waits |
   |---|---|
   | pip, 12 s | 0 to 1% |
   | aws, boto3, Railway CLI, 50 s | 2 to 10% |
   | unknown client, 90 s | 7 to 18% |
   | git push, terraform, a model reply kept alive, 10 min | 62 to 68% |

   An ask that stays open 30 minutes (#355) catches 84 to 94% of these answers. Over an evening
   then a night (3 hours away, then 8 hours asleep) it catches 25%, and an ask that stays open
   until answered catches 96% by the next morning. The ranges are a narrow and a wide spread
   around each published median; the model is not a measurement of people. (`results/model.json`)

4. **The late yes worked end to end with every client tried.**
   - `git push`, held 60 s, yes at 90 s: the push was refused in Git's own terms, with Silo's
     words on `remote:` lines; the same push at 120 s passed at once, with no second ask. A new
     commit pushed at 140 s opened a new ask. (`results/s2_push_late_yes/`)
   - boto3 `put_object`, held 90 s (past its 60 s): boto3 closed at 60 s and sent the request
     again with the same `amz-sdk-invocation-id`. The copy joined the open ask, the yes at 75 s
     let it pass, and one request reached the stand-in S3. (`results/s3_boto_hold90/`)
   - boto3 held 50 s (under its 60 s): it got the 403 and did not retry; the yes came at 70 s and
     the agent's rerun at 100 s passed once. (`results/s4_boto_hold50_late_yes/`)
   - Codex: the push returned "Process running" after 10 s with Silo's first line already in the
     output, the refusal came on the model's next `write_stdin` poll, and a second push after the
     yes passed with "polarzero said yes (ask_1). Pushing." (`results/a3_codex_late_yes/`)
   - Claude Code: a retry sent at once, before the yes, joined the open ask and passed while
     still held when the phone said yes at 90 s. (`results/a1_claude_code_late_yes/`)

5. **Claude Code 2.1.332 will not let a model wait and retry with `sleep`.** The scripted model
   sent `sleep 50; git push …` after reading the refusal, and Claude Code blocked the call itself:
   "Blocked: sleep 50 followed by: git push … To wait for a condition, use Monitor with an
   until-loop". So "run the same command again later", which Silo's refusal says, is up to the
   model's next turn or a Monitor loop, not a sleep. The ask path does not depend on it: a retry
   sent at once joins the open ask and waits there for the yes, as finding 4 shows.
   (`results/a0_claude_code_sleep_then_retry/`)

6. **In `claude -p`, the model did not learn that a backgrounded push passed.** Held 10 minutes
   with a yes at 200 s, the push went to the background at 120 s with Silo's `remote:` lines in
   the result, the model ended its turn, and Claude Code waited for the command and exited at
   201 s with the push done. No new model turn carried the result. #355 saw a notification turn on
   2.1.331; this run on 2.1.332 did not. Not tried in the interactive mode.
   (`results/a2_claude_code_yes_at_200/`, `results/agents/cc_wait.log`)

7. **An ask can list requests the person never thinks about.** `pip download` first fetched
   `/simple/pip/` (its own version check), gave up on it after 5 s, and then asked for the
   package. Both went into one ask. A burst of six deletes became one ask in six versions; the
   phone showed three of them as they settled, and the yes covered the version shown.
   (`results/s5_pip/`, `results/s6_burst_curl/`)

8. **A phone that buzzes for revocations is a phone people stop reading.** On #405's settled
   labels of #383's 1,085 documented writes, the rules as merged hold 43 of every 100 writes, and
   34% of those asks are wrong holds. Letting a revocation pass, as #405 proposes, takes it to 36
   per 100, with 20% wrong. These are documented operations, each counted once, not a night's
   traffic.

## What it changes in the plan

Notes only. No list, verdict or answer is changed.

- `hold-sentence`: the late yes measured end to end; the ask's own life weighed on the phone.
- `hold-limit-length`: on the phone, the hold's length decides little; most answers come after it.
- `ask-reaches-you`: an answer signed by Silo's phone app, carried by any relay, keeps "only Silo's
  own apps answer" true.
- `phone-app`: the way to reach the Mac from outside its network, prototyped.
- `what-silo-sends-home`: a push to an iPhone needs the app publisher's push key.
- `asks-for-a-burst`: a version the phone showed is what a yes covers; pip's version check.

## For you to rule

- **Who carries the phone app's push.** Apple delivers a push to Silo's iPhone app only from a
  server that holds Amont Labs' push key, so the phone app needs a relay Amont Labs runs. Even a
  self-hosted ntfy server sends its iOS wake-ups through ntfy.sh for this reason, and without it
  messages can be delayed for hours. Case: an agent pushes at 23:10 while you are out; your phone
  buzzes because Amont Labs' relay passed on a sealed message of 341 bytes. The relay learns that
  your Mac had something for your phone at 23:10, and nothing else. Options: Amont Labs' relay,
  only once you pair a phone (recommended); your own relay, which still goes through Apple via a
  service that holds a push key; no push, where the app checks when opened and an ask waits for
  you to look. `what-silo-sends-home` says Silo sends nothing on its own; pairing a phone would be
  turning it on. Notes on `what-silo-sends-home` and `phone-app`.
- **How long an open ask lasts.** `hold-sentence` (#391) keeps it open until you answer or set it
  aside; #355 proposed 30 minutes. Case: an agent's `terraform apply` is held at 01:00; you answer
  at 07:30. With 30 minutes the ask closed at 01:30 and the agent's next try opens a new ask; with
  no limit your yes lets that same apply through once when the agent tries again. Recommended: no
  limit, as `hold-sentence` says, since the yes is bound to one exact request and spent once.
  Notes on `hold-sentence`.

## The research at a glance

| Draft | Subject | What it found |
|---|---|---|
| this one | An ask that reaches a phone, end to end | Hold, ask, sealed push through an untrusted relay and a signed yes back worked with git, boto3, pip, Claude Code and Codex, with no port open on the Mac; 12 of 12 tamper cases refused. Away from the Mac, a 90 s hold is answered in time 7 to 18% of the time, so the late yes does the work; an ask open 30 min serves 25% over a night, one open until answered 96%. Claude Code 2.1.332 blocks `sleep; retry`. The phone app's push needs a relay holding Amont Labs' push key |

## How it was done, and what was not verified

- `proto/askd.py` plays the boundary (a Git server and stand-in APIs, holding pushes and writes),
  the asks (one open ask per computer, lines matched by fingerprint, late yes spent once), the
  forwarder (reads the record's control lines, as an extension would) and the answer reader.
  `proto/relay.py` is the relay, with a switch that makes it hostile. `proto/phone.py` is the phone
  app, with a fixed delay standing in for the person. `proto/link.py` seals and signs.
  `proto/rig.py` runs one scenario from `proto/scenarios/`; `proto/tamper.py` runs the tamper
  cases; `proto/model.py` the model.
- The agent programs ran against #355's fake model rig
  (`research/2026-11-24-gap-fixes/prototypes/holds/rig/`), so every step of the agent was
  scripted. What the programs did with each call is measured; what a real model would decide to do
  after reading Silo's words is not.
- The person is modelled from three published figures: a median of 6.67 s to approve a Duo push
  ([DuoLungo, 2026](https://arxiv.org/html/2602.01489v1)), a median of 6.15 min to attend a message
  notification (Pielot et al., CHI 2014, as cited in
  [Katevas et al.](https://www.arxiv.org/pdf/1612.02314v1)), and a mean response of 54 min, 27
  with scheduling (Okoshi et al., as cited there). The spread around each median is a guess.
- Push delivery: Apple publishes no delivery figures. A developer measured about 0.8 s mean and
  2 s at the 95th percentile for VoIP pushes, and another saw delays up to 5 minutes on poor
  connections ([Apple developer forums](https://developer.apple.com/forums/thread/825224),
  [another thread](https://developer.apple.com/forums/thread/762088)). On ntfy and iOS:
  [ntfy's known issues](https://docs.ntfy.sh/known-issues/) and [config](https://docs.ntfy.sh/config/).
- No real phone, push service or Mac was used. A Mac that sleeps holds nothing and reads no
  answer; the relay keeps the answer, and the prototype's answers expire 10 minutes after they are
  given, which would drop a yes given while the Mac sleeps longer. A real answer should expire
  with the ask, since a nonce and a closed ask already refuse a replay.
- Git printed `--negotiate-only requires protocol v2` warnings from this machine's settings; they
  change nothing measured.
