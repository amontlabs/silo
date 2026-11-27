# Numbered lines

How it works, as explained to polarzero: a line is one row of the record. Each source of lines
numbers its own as it produces them, before sending. If lines 4, 5 and 6 are lost on the way and
7 arrives, the recorder (the part of Silo, outside the computer, that keeps the record) knows
three are missing and writes a line of the kind "gap". A source that had nothing to say leaves no
gap. The numbers count what was produced, not what arrived.

## The alternatives, and the check (27 November 2026)

An agent's part. A second agent, as judge, read this entry on its own and found it is about how
Silo is built, so agents may take it once the entries it needs are taken (`owners-part-on-engine`).

- The recorder counts arrivals. It costs every loss before arrival.
- Time stamps only. It costs telling a quiet source from a lost line.
- A hash chain per source. It detects the same and costs more per line.
- Check: a bounded exhaustive test over drop, repeat, reorder, restart and silence, where every lost line gives a gap or a restart fact.
- Found by the judge: the answer writes a restart as a fact, and `kinds-of-lines` has no kind for it.
