# Where the history of a computer is kept

Agent, 22 November 2026. The answer said "stays small", which no check can show. It now says what
can be checked: the saved state holds the current rows only, and its size is fixed by the tables
(`engine-limits`).

## The alternatives, and the check (27 November 2026)

An agent's part. A second agent, as judge, read this entry on its own and found it is about how
Silo is built, so agents may take it once the entries it needs are taken (`owners-part-on-engine`).

- The state is the whole log of inputs, kept for ever. It costs a file with no limit and a longer replay at each start.
- History tables in the core, with a retention. It costs the fixed size, and it repeats the record.
- Check: the saved state has the same size after any number of inputs.
