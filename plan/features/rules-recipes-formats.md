## Proposed

- Every rule is set by picking among choices. Using Silo never requires writing code or a spec.
- Recipes and rules are short declarative data, which the engine checks against limits. Any logic is a script run through the command line.
- Every recipe, rule and setting can be shown and set as text through the command line. Silo keeps the stored copy.
- Silo is extended by a client of the engine's calls or by a program inside a computer. Nothing loads into the engine or the app at run time.
- Silo's own parts inside a computer are self-contained. They install no package or runtime the user did not choose.
- A file Silo writes in a computer is either Silo's, marked as such and rewritten freely, or the user's, and then never overwritten once edited.
- The wording of each rule a user can set is tested: two readers given the same sample cases predict the same outcome.
- Silo adds a file format only for a use someone asked for, and uses an existing format when one fits.
