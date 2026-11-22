# Now

## The choice

An agent's part, 22 November 2026. Three runs are alive in main. claude #1 has worked for 1 h 12 and
spent $6.40, codex started 8 minutes ago, and claude #2 has made no model call for 38 minutes while
it runs `sleep 600`.

The options:

- One line per run, with its state (recommended). claude #2's line says "stuck", shown as Silo's
  opinion, with what it rests on. It costs a wrong state now and then, such as a long build that
  looks stuck. That is why the state is shown as an opinion and never as a fact.
- One line per run, facts only. The line says "no model call for 38 min" and leaves the judging to
  you. It costs you reading every line to find the one that needs you.
- One line per computer. "main: 3 agents, $14". It costs knowing which of the three is stuck until
  you open main.

Recommended: one line per run, with its state. You act on a run, and a state saves you reading every
line.

## Notes

Marked Notes in the design record. The state of a run is given by the feature
`how-each-run-is-going`, in `opinions`.
