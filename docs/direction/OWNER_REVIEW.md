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

## Silo's tools inside a computer (T5b review)

6. **Silo marks its own MCP entry and never touches a user's entry named `silo`.** If the user
   registered their own `silo` server, Silo leaves it and shows a conflict instead (being built,
   R3). Why: overwriting a user's config silently is worse than a visible conflict.
7. **A computer that keeps failing its tools check is retried less, then not until its next
   start**, and one check has an overall time limit (R3). Why: a broken or hostile computer must
   not slow down tools for every other computer.
8. **"Tools OK" for a computer is the computer's own claim.** An agent with root inside can fake
   it; that is accepted, since the agent already controls that computer. Documented.

## App (T10, T13 review)

9. **Saving a computer's network settings never accepts a preset update.** Updates to a preset
   (for example npm adding hosts) are accepted only with their own Accept button, which shows
   the hosts they add (being built, R4). Why: Save used to accept them silently.
10. **Approvals show the computer's id beside its name** and strip invisible characters from what
    an agent wrote (R4). Why: an agent could name a computer to look like another one.

## Rebuild on the newest image (T15)

11. **A rebuild is refused when `/home/silo` is over 45% of the root disk** (about 1.8 GiB on a
    4 GiB disk), because the home is packed inside the computer before it moves.
12. **On a device whose disk cannot make instant copies, only layered disks are refused**; a
    single-file disk is copied normally (slower, uses space).
13. **The old computer stays, stopped, as `<name>-undo`**, so it can be inspected, restored or
    deleted with the usual commands.
