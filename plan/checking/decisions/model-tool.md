# Which tool checks the model of access

The model is small: who holds what, the calls that change it, and the properties whose `check` is
`model`. The tool tries the sequences of calls among a few computers, secrets and hosts, and shows
the shortest one that breaks a property.

## Found so far

| Tool | Runs on | Fact | Source, date |
|---|---|---|---|
| Quint | Node 18 or later | Version 0.33.1, five releases in twelve months | its releases, counted by the trial of 21 November 2026 |
| TLA+ with TLC | Java | Stable 1.7.4 | a research agent's report of 20 November 2026, not checked again |
| Alloy 6 | Java | Version 6.2.0 of 9 January 2025 | the same report, not checked again |

Java 25 and Node 26 are on the owner's Mac.

## Not done

No tool has been run on a model of Silo. A trial settles this: the same small model written for
each candidate. Two things decide, agreed with the owner on 21 November 2026:

- a person reads the model easily;
- the tool can check a history that the Zig simulator produced, so that the model cannot drift
  away from the core. `model-of-access-first` says the simulator compares the core with the model,
  and this is what makes that possible.

The trial runs on the branch `trial/model-tool`. Its candidates are the three above, FizzBee, and
the model written in Zig itself and run over every sequence of calls up to a size.

One fact for the second point, read on 21 November 2026 in Quint's own documentation
(quint-lang.org/docs/model-based-testing): `quint run --mbt --out-itf` writes histories as JSON
files, each step with the call that was taken. Libraries replay them against Rust code; none
exists for Zig.

## Found

A trial on 21 November 2026 wrote the same small model of access for five candidates, on the
owner's Mac. The model has five tables, seven calls and four properties. Its bounds are
2 computers, 2 secrets, 2 hosts, 3 rules and 6 calls in a row. Each candidate also had to write
histories to a file and check a history given to it, in one format: one JSON object per line.
The code is thrown away, as every trial's code is.

The fifth candidate has no separate model. It is a small core in Zig, run over every sequence of
calls, and it is described in the notes of `core-simulator`.

| | Quint 0.33.1 | TLA+ with TLC 1.7.4 | Alloy 6.2.0 | FizzBee 0.5.4 | The core in Zig 0.17.1 |
|---|---|---|---|---|---|
| Lines of the model | 251 | 161 | 226 | 210 | 322 for the core, 92 for the properties |
| Check at the bounds | 62 s with its symbolic checker. Its checker that visits every state stopped at 5 calls, in 390 s | 85 s, treating interchangeable names as one. Without that, 5 calls took almost 11 minutes | 28 s to 1,342 s for each property | not done: 5 calls took 1 h 20 min, and 6 would take 9 to 10 hours | 44 s, every sequence |
| Two faults put in on purpose | found, with the shortest history | found, with the shortest history | found, with the shortest history | found, with the shortest history | found, and reduced to the shortest history |
| Writes histories | 645 a second, through a converter of 36 lines | 28,000 a second, through 6 lines of awk | 35 to 50 a second, through a converter of 97 lines | 1 to 2 a second, through a converter of 171 lines | writes the histories it ran, with no converter |
| Checks a history | a script of 11 lines does it. Quint has nothing for it | reads the format itself, in about 0.55 s for one history | through the converter: 455 histories in 2 min 54 s | the converter compares the results. The tool exits with 0 when a history stops early | replays the file and says where it diverges |
| Adding one call | 6 lines | about 9 places, with one line in each existing call. With a new table, the check went from 85 s to 36 minutes | 11 lines | 11 lines | not measured |
| The tool today | released 28 October 2026. No release between 31 March and 28 September. A company maintains it | stable release of 5 August 2024. 1.8.0 is a pre-release of 19 November 2026, and the current library for JSON needs it | released 9 January 2025. 3 commits in twelve months | released 1 October 2026. 6 releases in twelve months, most commits by one account. Its tool that drives real code is closed | Zig only, nothing to install |

Most timings were taken while other trials ran on the same Mac. Read them as orders of magnitude.

Three of the agents found the same thing about the model, not about a tool. Once a call takes a
rule away, properties 1 to 3 fail as the statement words them, because the list of rules a request
saw is read against the rules of now. The statement assumed that no rule is ever taken away. The
properties of the part `rules` will meet this case when taking a rule away is designed.

Not done: a person reading the five models, which is the first of the two things that decide.
