# Which tool checks the model of access

The model is small: who holds what, the calls that change it, and the properties whose `check` is
`bounded exhaustive test`. The tool tries the sequences of calls among a few computers, secrets and hosts, and shows
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
each candidate, and the one a person reads most easily wins.
