## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes becomes a line of `plan/decisions.json`.

- Every rule is set by picking among choices, and using Silo never requires the user to write code or a spec.
  - Recommendation: yes. working-recipe already makes recipes guided, and the recipes decision moves rules into the app.
- Recipes and rules are short declarative data the engine checks against limits; any logic is a script run through the command line.
  - Recommendation: yes. The engine can check data but not code, and a recipe's setup lines stay commands it runs.
- Objection to engine-in-zig and mac-app-in-swift: users expect to add extensions while Silo runs, so the engine and app should load plugins at run time.
  - Recommendation: no. An extension is a client of the engine's calls or a program inside a computer, so nothing has to load into the engine.
- Every recipe, rule and setting can be shown and set as text through the command line, while Silo keeps the stored copy.
  - Recommendation: yes. It keeps the recipes decision, where Silo stores the recipe, and makes everything scriptable.
- Silo's own parts inside a computer are self-contained and install no package or runtime the user did not choose.
  - Recommendation: yes. Packages Silo pulled in would show in the diff and clash with the user's own.
- Every file Silo writes in a computer is either Silo's, marked as such and rewritten freely, or the user's, and then never overwritten once edited.
  - Recommendation: yes. Agent programs' settings files are written by both, and a user's edit must never be lost to an update.
- The wording of each rule a user can set is tested: two readers given the same sample cases must predict the same outcome.
  - Recommendation: yes. A model can be the second reader, and an ambiguous rule is a hole in the boundary.
- Silo adds a file format only for a use someone asked for, and uses an existing format whenever one fits.
  - Recommendation: yes. one-feature-at-a-time already holds this for features; formats are what is left.
