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

## Resources (R2)

14. **A start's memory is held for it until the start ends, at most 5 minutes**, and a start that
    asks for more than the free memory above the floor is refused. A start the owner approved
    checks the floor but does not hold memory, so several approved starts at once can still pass
    together. Left as is: the owner approved each one.

## Recipes and tools (R1, R3)

15. **A computer still sees each recipe's hash in the list.** It is a one-way digest of the
    recipe, kept so agents can tell when a recipe changed; it does not reveal setup lines.
16. **One tools check has 90 seconds in total**, which also caps sending the `silo` program
    (it used to have 170 s alone). Sending 13 MB inside the device is fast, so this should not
    bite; raise it if a slow computer shows "waiting" for its tools.
17. **After 5 failed tools checks in a row, Silo stops checking that computer until its next
    start.** The count is forgotten when the engine restarts.

## App and rebuild details (R4, T15 round 2)

18. **The app keeps the last 50 finished approval jobs in memory** to share their results
    between windows; older ones are dropped.
19. **A computer's network settings are never re-pinned to new preset versions by a plain Save**,
    with or without the compare-and-set check; only Accept does it.
20. **The "Before rebuild" checkpoint is kept** after a successful or rolled-back rebuild, as a
    way back on the same computer. Delete it by hand when no longer needed.
21. **While a rebuild runs, device-wide actions such as Quit or an update wait for it.**
22. **A rebuild on a computer already on the newest image is refused unless forced**
    (`silo rebuild --force`).
23. **Writes to `/home/silo` in the few seconds between packing it and stopping the old computer
    are only in `<name>-undo`.**

## Recipe setup lines (found by T16)

24. **A computer never sees a recipe's setup commands, not even for its own recipe** (being built,
    R5). Any computer may fork a recipe's hidden build to get a computer from that recipe, and the
    fork carried the recipe's full text, so the earlier "only its own recipe" rule could be
    sidestepped. Whatever a setup command writes to disk is in every computer built from that
    recipe anyway, so the docs now say: never put tokens in setup lines; use secrets.
25. **A recipe's `test` command and service names stay visible to computers**; only setup, service
    `run`, and the keep `migrate`/`after` commands are hidden, and a merge's report names a
    recipe hook step without its command text (R5). The docs say never to put a secret in any
    recipe line.
    A merge you approved also shows step numbers instead of commands in its report, since the
    asking computer reads the same report; the commands stay readable in the recipe.

## macOS disk size (R6)

26. **When Silo cannot make macOS use a larger disk, the computer keeps the larger disk file and
    says macOS can use only part of it**, instead of failing the copy. The record now always
    matches the disk file, so export works. The real fix (moving the Recovery partition out of
    the way before growing the disk, if that is the cause) is not scheduled; nothing retries the
    expansion later.

## From the end-to-end run (V)

27. **A computer may read its own Silo's tools status**, not other computers'. Refusals shown to
    agents use command words, not internal method names (being built, R8).
28. **Silo's own per-computer files (the channel endpoint, the tools receipt, ids and tokens) are
    never part of a merge**, so a fork's merge no longer conflicts on them (R8).
29. **One merge ask is one row on Pending**; its review opens from that row (R8).

30. **Blocked web requests from a computer get Silo's JSON "blocked by policy" answer instead of
    a refused connection** (MicroSandbox 0.7.14, upstream #1825). In allowlist mode a direct
    connection to an unlisted address on port 80 or 443 now completes, and the gateway answers
    it with a 403 itself; nothing is sent to that address. Silo keeps upstream's behaviour rather
    than carrying a patch to turn it off (branch `silo/0714-fix-net` has that patch if wanted).
    The live check found other ports behave the same way: a connection to an unlisted address
    completes but gets nothing back (DNS over TCP gets the gateway's own "no such name"), so a
    program sees a silent connection or a timeout rather than an immediate refusal. The live
    network test checks that no data comes back from the unlisted address.

## App timings (R13)

31. **Recovery and refresh timings:** while Silo cannot load or a check fails, it retries after 2,
    3 and 5 seconds, then every 10 seconds for about 5 minutes; the macOS list refreshes every
    3 seconds while a computer is being prepared and every 30 seconds otherwise; toasts with a
    button close after 20 seconds, quick ones after 4. Successful bookkeeping calls (job
    acknowledgements, status polls) are hidden from Activity; failed ones show.
