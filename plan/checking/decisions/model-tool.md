# Which tool checks the model of access

The model is small: who holds what, the calls that change it, and the properties whose `check` is
`model`. The tool tries the sequences of calls among a few computers, secrets and hosts, and shows
the shortest one that breaks a property.

## Found so far

| Tool | Runs on | Fact | Source, date |
|---|---|---|---|
| Quint | Node 18 or later | Version 0.33.1, six releases in twelve months | npm registry, read 20 November 2026 |
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
