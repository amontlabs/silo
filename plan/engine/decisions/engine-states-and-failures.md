# How the engine's code keeps a failure from being forgotten

The owner said this on 19 November 2026. What Zig gives is agreed as the reading.

The rest of the same line has its home elsewhere: "Bad paths are written first" is the principle
`bad-paths-first`, put forward by `bad-paths-principle` and not in force; "A bad state should be
impossible to write down" is the principle `no-bad-state`, put forward by `bad-state-question` and
not in force; a precaution's type and the table of the kinds of action are
`precaution-grants-nothing` and `every-action-has-a-kind`, in the part `rules`.

## Found

A trial on 21 November 2026 built a small core in Zig 0.17.1. It is described in the notes of
`core-simulator`. One state and one failure were then added to the core, to see what the compiler
refuses.

| Code | Compiler |
|---|---|
| A `switch` over the states or over the failures, with no `else` | refuses |
| A function returns a failure that the set it names does not list | refuses |
| A result that can fail is ignored, or thrown away with `_ =` | refuses |
| A `switch` with an `else` branch | lets it through |
| `catch unreachable` | lets it through, and the program stops when the failure happens |
| `catch null`, which turns every failure into one value | lets it through |
| A comparison of tags, such as `!= .running` | lets it through |

One real case from the trial: the core tested a computer with `!= .running`. Once the state
`paused` existed, a paused computer was refused as dropped, and nothing said so.

So the compiler refuses code that forgets a case only where the code is written a certain way.
The trial proposes to say so in the answer. In the core:

- a state is read by a `switch` with no `else`, never by comparing tags;
- a `switch` over failures has no `else`;
- there is no `catch unreachable`.

A test that reads the core's source would refuse each of them. That test was not built. A test of
the same kind was built for `deterministic-core`, with the parser that comes with Zig.

Not put to the owner yet.
