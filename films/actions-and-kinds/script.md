# Chapter script: actions and their kinds

This is the second of four films that replace `record-four-layers`. It comes after
`what-silo-knows` and before `rules-and-precautions` and `added-classifiers`.

## Who watches

The owner of Silo. He builds the product with agents and decides everything. He has no computer
science degree and learns from strong engineers. He wants to understand how the product is put
together, one piece at a time. So: plain words, and every term is shown as a concrete thing the
first time the voice says it.

Length: short and clear, with no hard limit. A scene takes the time its idea needs.

## What he should understand at the end

What can happen to one thing at the boundary (allow, hold, deny), where the rules that decide it
are kept, how a hold ends, how that differs from what happens to a whole computer (pause, stop),
and that every action has one of three kinds.

## Title and answer line

- Title: Actions and their kinds
- Answer: At the boundary it allows, holds or denies. Every action is granting, destructive or reversible.

## Marks used

- Agreed: put forward in the discussions of 18 and 19 November 2026 and accepted by the owner. Not yet recorded as a decision.
- Notes: proposed in the plan's notes before these discussions. The design relies on it and keeps it. It is still a proposal.
- Owner: said by the owner, as an idea or a direction.
- Proposed: Claude's proposal. The owner has not answered it.

## Statements (shown under the film, each with its mark)

1. [Notes] The boundary is the edge of a computer. A channel is one way through it: the network, ports, files, secrets, the screen, the clipboard, repository access. None exists by default.
2. [Agreed] One thing waiting at the boundary is allowed, held for your answer, or denied. Everything is denied unless a rule allows it.
3. [Notes] A hold makes only one thing wait: the thing it is about. The computer keeps running. Your yes is a grant.
4. [Agreed] Rules have no order. Deny beats hold, hold beats allow, and no rule means deny.
5. [Agreed] Deny forbids one thing and the computer keeps running. Pause and stop act on a whole computer.
6. [Agreed] Every action has a kind. Granting: a computer can do something it could not just before. Destructive: it ends or erases something, and that cannot be undone. Reversible: neither, so a wrong one costs only time.
7. [Owner] Anything added to Silo says the kind of each of its actions before it is designed.
8. [Owner] A hold always ends: by your answer, when the program closes its connection, or at Silo's own limit.
9. [Proposed] A thing also passes when you say yes to an ask. Your yes is a grant that only you can make.
10. [Proposed] Stop, restore and drop are destructive because each ends the programs that were running, which no checkpoint keeps. The disk is not lost.
11. [Notes] A rule is enforced by the engine, outside the computer. Nothing done inside a computer can change its rules.
12. [Agreed] Rules that live inside an agent program are not a boundary.
13. [Proposed] An alert is one more action, never a decision at the boundary. A rule that only alerts decides nothing there: the thing is allowed, held or denied by the other rules, and denied when no rule allows it.
14. [Proposed] Taking back a secret and closing a network are filed as reversible. If they are, a precaution can do them: an opinion alone could cut a computer's network or take a secret away from it.

## Words introduced (shown under the statements)

- boundary: the edge of a computer. Everything that enters or leaves it passes through parts of Silo that run outside it.
- channel: one way through the boundary. None exists by default.
- rule: data you set. At the boundary it allows, holds or denies. The next film says what triggers it.
- allow, hold, deny: what happens to one thing waiting at the boundary: it passes, it waits for your answer, or it is refused.
- ask: what a hold shows you: a request that waits for your answer.
- grant: to give a computer the ability to do something. Its opposite is to revoke. The words come from access control.
- alert: what Silo raises for your attention.
- pause: freeze a whole computer: nothing in it runs, nothing crosses its boundary, and nothing is lost. Undone by resume.
- stop: end the programs running in a computer. Its disk stays.
- checkpoint: a saved moment of a computer. It never changes once saved.
- restore: put a computer back to a checkpoint.
- fork: a copy of a computer, or of one of its checkpoints, that then lives its own life.
- drop: remove a fork. It goes to the bin first.
- bin: where a dropped fork waits for a time before it is removed for good.
- secret: a value a computer can use and cannot read.
- granting, destructive, reversible: the three kinds of action.

## How things look

Keep the look of `what-silo-knows`: the computer "main" and its edge, the record list, the marks
on its rows. A red cut is used for one thing only: a deny at the boundary. A denied link is drawn
only as far as the boundary and never reaches its host, and nothing is seen passing. A paused computer is still and pale, with its programs frozen in place. A
stopped computer is dark, and its programs are gone.

## Scenes

Six scenes. Each scene's `source` is the statement with the same number, word for word.
Statements 7 to 14 are spoken or shown inside scenes 2, 3, 4 and 6 and are listed for him to
mark. Captions are ten words or fewer. The narration is what the voice says; it is also the
subtitle.

### Scene 1. Caption: "The boundary, and the channels through it."  Source: statement 1

Narration:
1. "A computer has an edge: the boundary. Everything that enters or leaves passes through Silo there."
2. "Each way through is a channel: the network, ports, files, secrets, the screen, the clipboard, repository access."
3. "None exists by default. You add each one."

Picture: a computer called "main" (a region) with an agent inside, and its edge drawn as a clear
line. As the voice says "passes through Silo there", a thin band along the outside of the edge
lights, with the one word "Silo" standing on the band itself, away from the list of channel names,
so that it does not read as the title of that list. The channels sit on that band. As the voice names them, seven faint dashed outlines appear along the edge, each with its
name: network, ports, files, secrets, screen, clipboard, repository. They are places where a
channel could be. None is there yet, and the edge behind them is unbroken. On "You add each one",
one of them, network, becomes a solid gate: now it exists. Hero moment: the edge with its seven
named places, and the one channel that exists.

### Scene 2. Caption: "Allow, hold or deny."  Source: statement 2

Narration:
1. "You set rules. Silo keeps them outside the computer, and nothing inside can change them."
2. "Rules that live inside an agent program are not a boundary: the agent can get around them."
3. "When something reaches the boundary, one of three things happens."
4. "It is allowed, and passes. It is held, and waits for your answer. Or it is denied, before anything passes."
5. "Whatever no rule allows, and you have not said yes to, is denied."

Picture: the network channel, large. A small list beside it: "Allow: crates.io" and "Hold:
api.stripe.com". Three attempts, one after the other, each as the voice says its word. A link to
`crates.io` is drawn and flows: allowed. A link toward `api.stripe.com` stops at the boundary, with
a small timer at its tip. A chip "ask" travels to you along a dashed link that starts from the
frame of the channel, not from the tip of the held link: held. The held thing itself goes nowhere. A link toward `paste.rs` is drawn only as far as the boundary
and is cut there in red. It never reaches the host, and nothing flows along it: denied. Under the third, the words "no rule allows it". Hero
moment: the three outcomes side by side.

Added to that picture (the rest of the scene stays as it is):

- The two rule cards, "Allow: crates.io" and "Hold: api.stripe.com", are on screen from the start
  of the scene, outside the computer. On "outside the computer", the small words "kept outside
  the computer" appear beside them and stay.
- On "nothing inside can change them", a chip `change a rule` appears inside "main", near the
  agent, grey and struck through. It never leaves the computer, and it stays.
- On sentence 2, a small card "agent program's own rules" appears inside "main", with the small
  grey words "not a boundary" under it. It fades out before the three attempts begin, so the
  held frame is not crowded.
- The three attempts then play on sentences 3 to 5 exactly as they do today on sentences 1 to 3.

### Scene 3. Caption: "A hold waits for you, and always ends."  Source: statement 3

Narration:
1. "A hold makes only one thing wait: the thing it is about. The computer keeps running."
2. "What you see is an ask. Your yes is a grant. The ask says exactly what it lets pass."
3. "And a hold always ends. If you do not answer, Silo ends it at its own limit. The thing is denied, with an error the agent program can read."
4. "If the agent program gives up first and closes its connection, Silo sees that too."

Picture: the held link toward `api.stripe.com`, stopped at the boundary, with a small timer on it.
Inside "main" two programs, `cargo build` and `tests`, keep running: their dots stay lit, also in
the held frame. An ask card stands beside you: "main wants to reach
api.stripe.com", with "yes" and "no". You, the ask and the lit "yes" stand over the left ending only. Over the right ending the same ask
stands unanswered, with the timer. Then two endings, side by side. Left: you choose yes, and the
link flows. Right: the timer runs out, the link is cut at the boundary before anything flows, and a
chip `error: held too long` goes back into the computer. Under the right ending, small: "your no
ends the same way". On the last sentence a third, smaller ending appears beside them: inside the
computer the agent program lets go of the held link, the link fades from the inside, and the words
"the program gave up: Silo sees it" stand at the boundary. Hero moment: the endings side by side.

### Scene 4. Caption: "Rules have no order."  Source: statement 4

Narration:
1. "Rules have no order."
2. "If one rule allows and another denies, deny wins. Hold beats allow. With no rule at all, it is denied."
3. "So the order in which you wrote them never changes the result."
4. "An alert is one more action, never a decision. A rule that only alerts leaves the thing to the other rules."

Picture: two rule cards, side by side, with no numbers and no arrows between them: "allow
registry.npmjs.org" and "deny packages younger than 7 days". A request chip
`get left-pad (2 days old)` goes toward the registry and stops at the boundary. Both cards light
up in the plain ink colour, not in the blue of an allowed link, the deny card wins, and the request is cut there in red. Nothing reaches the registry. Then the two cards swap
places, the same request arrives again, and the result is the same. Under them, small:
`deny > hold > allow`, and `no rule = deny`. Hero moment: the same result after the swap.
On the fourth sentence, a third small line appears under `deny > hold > allow` and
`no rule = deny`, in the same style: `alert = one more action, not a decision`, with a quiet mark
"proposed" beside it. It stays.

### Scene 5. Caption: "Deny: one thing. Pause and stop: the whole computer."  Source: statement 5

Narration:
1. "A deny forbids one thing, and the computer keeps running."
2. "Pause and stop act on the whole computer. Pause freezes everything, nothing is lost, and you can resume."
3. "Stop ends the programs that were running. The disk stays."

Picture: three copies of "main" side by side, each with the two programs `cargo build` and `tests`
inside, appearing one by one. Each label, "deny", "pause", "stop", appears with its change and not
before.
First, "deny": a link toward `paste.rs` is drawn only as far as the boundary and cut there in red.
It never reaches the host, and the two programs keep moving. Second, "pause": the computer is still and pale, both programs
frozen in place, with a small "resume" control. Third, "stop": the computer is dark, the two
programs are gone, a small disk mark remains, with a "start" control. All three stay in the held
frame. Hero moment: the three computers side by side.

### Scene 6. Caption: "Every action has a kind."  Source: statement 6

Narration:
1. "Every action Silo can take has a kind."
2. "Granting: the computer can do something it could not just before."
3. "Destructive: it ends or erases something, and that cannot be undone."
4. "Stop, restore and drop are here because each ends the programs that were running. The disk is not lost."
5. "Reversible: it grants nothing and destroys nothing. A wrong one costs only time."
6. "Two of these are still for you to agree: taking back a secret, and closing a network. If they are reversible, a precaution can do them."
7. "Anything added to Silo says the kind of each of its actions first."

Picture: three columns appear one by one, each as the voice names it, each with its actions as
chips. The three headings are in the same colour. Amber is kept for what needs your attention: a hold,
an ask, an alert. Granting: allow, resume, start, fork, give a secret, open a port or any other channel, let
a classifier read a record. Destructive: stop, restore, drop. Reversible: alert, checkpoint, hold,
deny, pause, take back a secret, close a network or any other channel. Under the destructive
column, small: "each ends the running programs; the disk is not lost". Under the granting column,
small: "your yes to an ask: only you". Under the reversible column, small: "if these two are
reversible, a precaution can do them". Six things carry a quiet mark "proposed", because the owner has not agreed them yet: the chips
`start` and `take back a secret`, the whole chip `close a network or any other channel`, which is
drawn in one tone because all of it is proposed, the words "or any other channel" in the granting
column, the line "your yes to an ask: only you", and the line under the destructive column, whose
mark stands under both of its lines. The chip `let a
classifier read a record` stays without a mark. The three columns stay on screen at the
end. Hero moment: the three columns complete.

## Kind of chapter: decides

The statements are choices about the product that the owner has opinions on. Each keeps its three
buttons: agree, change, leave open, with a note.

## Was it clear? (one line per scene: "clear" or "lost me here", with a field for why)

## Test the picture (he answers in a field, then reveals the answer)

Question: One rule allows main to reach github.com. A second rule denies anything that carries the
secret named deploy-key unless it goes to deploy.example.com. An agent pushes to github.com with
that key. What happens, and does it matter which rule you wrote first?

Answer: The push is denied. One rule allows it and another denies it, and deny beats allow. The
order in which you wrote the rules does not matter: rules have no order. The computer keeps
running, because a deny forbids one thing only.

## A choice

None for this chapter.
