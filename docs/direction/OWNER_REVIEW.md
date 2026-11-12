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

5. *(Superseded by 38.)* **Starts admitted but not yet booted reserve their memory**, and a request larger than the
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

14. *(Superseded by 38: approved starts reserve too.)* **A start's memory is held for it until the start ends, at most 5 minutes**, and a start that
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

30. **A computer in allowlist mode gets an immediate refusal for any address it may not reach,
    on web and DNS-over-TLS ports** (owner decision, R20). MicroSandbox 0.7.14 (upstream #1825)
    had made a connection to an unlisted address on port 80 or 443 complete so the gateway could
    answer with a 403, and DNS over TLS (853) completed too; a program saw a blocked-request page
    or a silent connection. Silo's build now refuses them (connection refused) and nothing is
    ever sent to the unlisted address. Two patch changes: the readable-denial default goes back
    to off (`http.deny_response`), and a connection to port 853 of any address other than the
    computer's own gateway is checked against the policy like every other port (the gateway
    can't complete TLS as another resolver, so it would only hang). Patch `refuse-unlisted`; the
    live network test expects "connection refused" at once on 80, 443 and 853.
    - **DNS to another resolver** (`dig @8.8.8.8 ...`, UDP and TCP 53 alike) is answered by Silo's
      own resolver: allowed names resolve, unlisted names get "no such name", and nothing leaves.
      DNS over TLS to another resolver is refused.
    - **Side effect:** the same setting governs the JSON answer for a secret sent to a host it
      isn't allowed on; that connection now closes silently (the 0.7.6 behaviour) instead of
      getting a JSON 403.
    - **Known leftover:** computers created by an earlier 0.7.14 build saved
      `deny_response: true` and keep the 403 on ports 80/443 until they are recreated; computers
      migrated from 0.7.6 are fine. No code change.
    - Upstream's behaviour stays available by setting `deny_response` explicitly.

## App timings (R13)

31. **Recovery and refresh timings:** while Silo cannot load or a check fails, it retries after 2,
    3 and 5 seconds, then every 10 seconds for about 5 minutes; the macOS list refreshes every
    3 seconds while a computer is being prepared and every 30 seconds otherwise; toasts with a
    button close after 20 seconds, quick ones after 4. Successful bookkeeping calls (job
    acknowledgements, status polls) are hidden from Activity; failed ones show.

## Allowlist wildcards (R16)

32. **Shared-hosting wildcards are allowed with a warning** (owner decision, R19): `*.netlify.app`,
    `*.github.io`, `*.cloudfront.net` and the like are saved with a plain warning ("lets this
    computer reach any site hosted there"); wildcards over a whole domain ending (`*.com`,
    `*.co.uk`) stay refused. A secret only opens such a wildcard once you tick "Let computers reach
    every site under these" in its editor, or approve a computer's ask that showed the warning;
    secrets saved before R19 stay as they were until then. Gap: hosts that only the full Public
    Suffix List knows (e.g. `*.surge.sh`) get no checkbox in the editor and the CLI has no flag,
    so for those you add the wildcard to the computer's network list directly.
33. **Guest output limits:** a command the engine runs in a computer is stopped when it prints
    more than 64 KiB of errors (helper runs) or 960 KiB (other runs); nothing it prints is
    written to the device's disk.

## Approvals seen by a computer (R17)

34. **Size never asks (owner decision, reverses R17 and the earlier create rule):** a computer
    forking, or creating, a computer larger than itself no longer needs your approval for its
    size, including a recipe's hidden build. The agent chooses from what its tools show (the
    `resources` tool lists the device's free memory, disk and CPUs, plus the sizes of the
    computers it can see). A computer that doesn't fit in free memory is still refused or
    queued. Container computers still ask (less isolated). Any policy on size, and a
    resource-usage pane, come later. I extended your fork decision to creation myself, since the
    same reasoning applies; say if creation should keep asking. Gap: the `resources` tool shows
    a computer only the computers it can see, so it cannot read the size of a recipe's hidden
    build before forking it, and it shows no per-computer disk size.
35. **What a computer sees of an approval:** a merge approval shown to the computer counts the
    recipe's setup steps ("runs 1 step last") instead of quoting them; you still see the full
    commands. A merge waiting for you shows the computer "A merge waiting for the approval of
    the person who runs Silo."

## Undo computer's network policy after a rebuild (R15)

36. **The undo copy keeps the policy the computer had at the swap:** a policy you change during
    the rebuild reaches the `-undo` computer; a change landing in the second or two of the swap
    itself does not (it keeps the policy from one change earlier, which you had approved).
37. **Rebuilds interrupted by an older Silo close the undo copy:** if an older Silo stopped a
    rebuild after publishing it but before saving the undo policy, this Silo closes the `-undo`
    computer instead of guessing its allowlist from the disk's recorded rules; you reopen it by
    hand. Rules read from a disk with no saved policy also hold every list at its first version.

## Memory and job limits for computers (R18)

38. **How long memory stays set aside for a job:** a job's memory reservation ends when the job
    ends, when the computer it boots is seen running, or 2 hours after the job starts its work
    (a backstop for a job whose worker died). A creation still queued more than 2 hours behind
    others loses its reservation; a restore-image download over 2 hours too. Is 2 hours right?
39. **Restart, undo and restore of a running computer need no free memory** (owner decision, R19):
    the computer's own memory stays set aside while it is stopped mid-restart, so nothing else
    can take it; only a larger saved state needs the difference.
40. **Deleted computers' jobs:** finished jobs of a computer that is gone for two housekeeping
    passes (1-2 minutes) are acknowledged for you at once, so they stop counting against the
    512-job limit; you can't read their results afterwards.
41. **Device audit rows** have their own share (256 waiting rows), so busy computers can't push
    them out of the audit log.
