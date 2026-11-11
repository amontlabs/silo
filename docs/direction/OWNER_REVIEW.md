# Direction build: for the owner to review

Choices made during the build without asking, and things worth a second look. Each says what
was chosen and why; any can be changed. Older items are also in the build log under
"Decisions taken while planning" and the "for the owner" notes under "Found during the build".

## Recipes (T6 and its review)

1. **A merge changes a saved recipe only when the owner runs or approves it.** An agent that
   merges into a computer it made updates only that computer's record. Why: otherwise an agent
   could add packages that the owner's next rebuild installs on every computer from that recipe.
2. **Undoing a merge also removes what it added to the recipe** (being built, R1). Why: after an
   undo the computer and its recipe should agree; a later rebuild should not bring back packages
   the undo removed.
3. **A computer sees only the recipe it was built from in full; other saved recipes show their
   names and descriptions only** (being built, R1). Why: recipe setup lines may hold tokens the
   owner pasted; an agent still needs the names to fork from a recipe.
4. **Caller-supplied merge hooks are refused for a computer built from a recipe**; the recipe's
   hooks run and the approval shows them.

## Resources (T3 follow-up)

5. **Starts admitted but not yet booted reserve their memory**, and a request larger than the
   free headroom is refused rather than admitted (being built, R2). Why: a burst of starts could
   each pass the floor before any computer took memory.
