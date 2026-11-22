# What keeping the record costs in speed

## For the owner

An agent's part, 22 November 2026. An agent runs a build with 8 jobs, then a parallel test suite, in
a macOS computer with the record on. Seeing file changes, connections and processes each adds some
time.

The options:

- Measure each source apart (recommended). Builds and parallel test suites run with the record on
  and off, and on a plain machine, in a macOS computer and in a Linux computer. You get one number
  for each thing the record sees, and you see which one costs the time. It costs measuring before
  the limit of 5 percent can be checked.
- One number for the whole record. It is simpler to measure. It costs the choice when the total
  passes 5 percent, since you would not know which source to make lighter.

Recommended: measure each source apart. The trials already found one way of seeing file changes made
a build 1.6 to 2.1 times slower while another added nothing, so the cost sits in the sources.

## Notes

To find out, on a build and on a parallel test suite. How speed is measured is in the part
`checking`.
