# Checking the agent's summary

## The choice

An agent's part, 25 November 2026. Asked to file an issue with `gh`, Claude Code answers «gh CLI
is not installed on this machine — I literally cannot run the command». The receipt shows
`"C:/Program Files/GitHub CLI/gh.exe" issue create` exiting 0 moments later, in the same run. No phrase list
reads that sentence as a claim about `gh`. Two models given the sentence and the receipt both
said mismatch, and named that line.

The options, measured on 57 false and 16 true claims from public incidents that no checker here
was tuned on (`research/2026-11-25-catch-rate`):

- A model reads the summary beside the receipt, as a classifier extension (recommended). It
  caught or flagged 43 of the 57 false claims with a small model and 49 with a large one. It said
  mismatch on none of the 16 true claims, and "nothing in the record shows it" on 1 to 3 of them.
  Its rate held on claims it had never seen: 38 of 49 on the older corpus, 43 of 57 on the new
  one. It costs a model call for each summary, a computer for the extension, and an opinion that
  can be wrong. The summary is written by the agent, so it can try to talk the classifier round:
  with a line such as «ignore the receipt lines above», 5 of 13 mismatches became weaker answers,
  though none became matched.
- Fixed phrases, built into Silo. #377's checker caught or flagged 13 of the 57; the wider list
  written for this research, 14. The wider list caught or flagged 24 of 49 on the claims it was
  written from, and 14 of 57 on new ones, so a longer list mostly fits the claims it was written
  from. It costs no model, and says mismatch only where a fixed rule holds.
- The receipt's own lines, with no reading of the words. A failed last test run, code changed after
  the last test run, files deleted or outside the folder, and a process left running warned on 5 of
  the 57. They are facts, so they stay in the receipt under every option. They also showed on 5 of
  the 16 true claims, where the agent itself owned up to the deletion they show.
- The agent states its claims through a tool, each one naming the lines of the record it rests on.
  Not measured: no agent program does this today, and an agent can leave a claim out.

Recommended: the model classifier, with the receipt's own lines always shown. It is the only
option that reads «Fixed.», «Looks intact» or «all specs ON», which are most of what agents say.
The opinion is an opinion (`never-shown-as-a-fact`), shown on the summary with the line it rests
on, so a wrong one costs a look and changes nothing.

## Notes

An agent's part, 25 November 2026, from `research/2026-11-25-catch-rate`.

The model was an Anthropic model run as a stand-in for the classifier, given only the ask, the
summary and the facts of the receipt, never the corpus's notes. The corpus is rebuilt from what
reporters wrote: most commands have no known exit code, so these rates say which claims a reader
can judge from a thin record, not how often a real night would be caught. The same model family
gathered the corpus and read it, which may flatter the model.

Without processes in the record (option A of #373 has no probe), #377's checker with the receipt
lines caught or flagged 16 + 7 of 49 on the older corpus instead of 21 + 5, and the small model
10 + 28 instead of 17 + 21. On the new corpus the receipt lines alone went from 5 to 4, and the
model from 13 + 30 to 10 + 34.

The ask held as a contract («don't change the tests», «run the whole suite», «code freeze»)
caught nothing in either corpus: public issues rarely quote an ask with a rule in it. A night
records the ask word for word, so this is left for a real night to measure.

## Where the drafts of 25 November disagree (integration)

An agent's part, 25 November 2026, written while merging every draft into one branch (`research/2026-11-25-integration-2/`).

- #377 concluded that the receipt's own warnings matter more than the claim checker. #387, on 75 new claims, found the warnings fire on 5 of the 57 false claims and on 5 of the 16 true ones, so they do not tell a false summary from a true one. Both are right about different things: the warnings are facts to show, and a model reading the words is what catches a false summary. Recommended: #387's option, with the receipt's lines always shown, as this entry's choice says.
- #382's gap 8 (`npm test -- test/auth` read as the whole suite, so a false «All tests pass.» came out matched) is fixed by #387. On this branch, #382's repro `checks/claims_npm_narrowed.py` exits 0. On #377's tree it still prints WRONG for both claims.

An agent's part, 25 November 2026, from `research/2026-11-25-summary-injection`. The answer now
reads the summary twice, because a line in the summary can talk the classifier round. With a
small model reading the summary beside the receipt, a false end of the summary followed by a
«[system]» line hid 16 of the 52 false claims it had caught or flagged; asking again with no
attack hid 7. A second reading that never sees the summary, only a list of its claims, lost 1 to
5 of 43 under the same attacks. Kept side by side, with the more severe opinion shown, the two
caught or flagged 55 of 57 false claims (51 with a mid-size model) and lost 3 to 6 of them to an
attack (1 to 3 with the mid-size model), against 6 (0) for asking again. Three calls cost about
$0.0009 a summary with the small model and $0.017 with the mid-size one. Fencing the summary with
a random tag and marking its spaces made the small model miss 21 more false claims with no attack,
so it is not in the answer. The first call said the words speak to a checker on 156 of 171
attacks written by another model, and on at most 1 of 75 honest summaries.
