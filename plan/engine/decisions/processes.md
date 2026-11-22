# Which processes make up Silo

| Process | What it holds | If it stops |
|---|---|---|
| **Engine** (core and server) | State and decisions | Running computers keep running under the rules they had. Changes wait. A hold still ends at its limit, which the helper keeps. |
| **Helper**, one per computer | The virtual machine and its boundary: the network stack with rules, secrets and reading; the disk; the screen | That computer stops. Nothing else does. |
| **Recorder** | The record, the built-in classifiers, the precautions | Facts and reports wait, then gaps. Rules are not affected. Opinions and precautions stop, and so do their alerts. A request that waits for a synchronous precaution still ends its wait at the helper's limit. |
| **App**, command | Views | Nothing stops. |
| **Extension's computer** | Its own work | Its answers and alerts stop. |
| **Probe**, inside each computer | Nothing Silo relies on | Reports stop, and a gap is recorded. Facts and rules are not affected. |

- That the recorder is its own process is put forward in `recorder-process`, and that the boundary
  lives in the helper in `which-process-holds-the-boundary`. Both need this decision and say what
  the table assumes.
