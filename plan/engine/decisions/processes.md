# Which processes make up Silo

## For the owner

An agent's part, 22 November 2026. Main and two forks are running. The part of Silo that runs one of
the forks hits a bug and crashes.

The options:

- Separate programs (recommended). The engine is one program, each computer has a small program of
  its own that runs it and holds its boundary, and another program keeps the record. Only that fork
  stops. Main, the other fork, the engine and the record carry on, and a computer's secrets sit only
  in its own program. It costs memory and processor for one more program per computer, not measured
  yet. It also costs more pieces that can fail one by one: when the program that keeps the record
  stops, the record gets a gap and precautions stop until it is back, while rules still apply.
- One program for everything. Less memory, and fewer pieces to start and to watch. It costs every
  computer at once on any crash, and one place that holds every computer's secrets.

Recommended: separate programs. An agent is treated as hostile, so the program that faces it should
hold one computer and nothing more. The entry also sets the words for two of these programs.

## Notes

| Process | What it holds | If it stops |
|---|---|---|
| **Engine** (core and server) | State and decisions | Running computers keep running under the rules they had. Changes wait. A hold still ends at its limit, which the helper keeps. |
| **Helper**, one per computer | The virtual machine and its boundary: the network stack with rules, secrets and reading; the disk; the screen | That computer stops. Nothing else does. |
| **Recorder** | The record, the built-in classifiers, the precautions | Facts and reports wait, then gaps. Rules are not affected. Opinions and precautions stop, and so do their alerts. A request that waits for a synchronous precaution still ends its wait at the helper's limit. |
| **App**, command | Views | Nothing stops. |
| **Extension's computer** | Its own work | Its answers and alerts stop. |
| **Probe**, inside each computer | Nothing Silo relies on | Reports stop, and the recorder writes that the probe fell silent. Facts and rules are not affected. |

- That the recorder is its own process is put forward in `recorder-process`, and that the boundary
  lives in the helper in `which-process-holds-the-boundary`. Both need this decision and say what
  the table assumes.
