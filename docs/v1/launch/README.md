# Silo v1 launch: checklist

Drafts for the 0.13.0 launch. Nothing here is public; the owner reviews each item before it is
promoted.

## Ready for review

| Item | File | State |
| --- | --- | --- |
| Top-level README | [README-draft.md](README-draft.md) | DRAFT. Install section names `Silo-macos-arm64.dmg` from the `v0.13.0` release, the unsigned first-open steps, Apple silicon and macOS 15+, the `silo` link on PATH and `silo mcp` setup. Every number names its file in `docs/v1/evidence/final/`. |
| 60-second demo | [demo-script.md](demo-script.md) | DRAFT shot list from the final run (D1 to D6). Not recorded. |
| Numbers post | [numbers-post.md](numbers-post.md) | DRAFT. Numbers copied from `docs/v1/evidence/final/final-run.txt`, with method, caveats and what was not measured. |
| Release notes | [`docs/releases/0.13.0.md`](../../releases/0.13.0.md) | Written by `release:version`, with the "Opening this version" paragraph. |
| Release procedure | [`docs/v1/release/README.md`](../release/README.md) | The owner's release steps, in order. |

Wording rules the drafts follow: the credentials sentence is exactly "Forks get none of the
credentials Silo manages unless you name them; files on disk are copied." The credentials-used
line records secrets Silo substituted; an ordinary secret's placeholder sent to another host
passes through unrecorded, and blocked lines come from the GitHub profile and a few edge cases
([`docs/SiloUI-SECRETS.md`](../../SiloUI-SECRETS.md)).

## What the owner does to launch

1. **Publish v0.13.0.** Follow the owner's steps in
   [`docs/v1/release/README.md`](../release/README.md#owners-remaining-steps-in-order): draft,
   acceptance evidence, then publish. Check that the release page carries
   `Silo-macos-arm64.dmg` and that
   <https://github.com/amontlabs/silo/releases/tag/v0.13.0> resolves.
2. **Swap the README.** Review [README-draft.md](README-draft.md), remove its DRAFT note and
   the "pending publication" note in Install, fix the relative links for the repository root,
   and replace the top-level `README.md` with it.
3. **Record the demo.** Follow [demo-script.md](demo-script.md) on the published production
   app, with the production `silo` command. Settle the shot marked as not in the final run
   (record it or drop it), and keep the "time cut" markers.
4. **Post the numbers.** Review [numbers-post.md](numbers-post.md), remove its DRAFT note and
   publish it. Do not add numbers that are not in `docs/v1/evidence/final/`.

Also worth a look before posting: the 0.13.0 release notes say `silo status` and `silo diff`
show "how many attempts were blocked"; with the passthrough behaviour above, that count covers
the GitHub profile and edge cases only.
