## Proposed, not confirmed by the owner

### For the owner to settle

- Do computers start only from Silo's own Ubuntu image, or from other distributions and arbitrary images too?
  - Recommendation: Only Silo's image, because a controlled base is what makes forks and 800 ms starts possible; recipes add the rest.
- Does the base image ship Docker and the common package managers ready to use, or stay minimal and leave them to recipes?
  - Recommendation: Ship them: macOS computers come with Git and Node (macos-tools), mac-and-linux-consistent asks for the same contents, and recipes arrive three stages later.

### For whoever builds it

- Test: In a new Linux computer, Docker, compose, bubblewrap and the common package managers, pnpm included, work with no setup, and a test suite that needs Docker passes.
- Detail: The base image includes a general scripting language, so an agent whose only tool is running a command can script everything else.
- Test: A shell in a new computer reports true colour through COLORTERM, and agent programs such as Claude Code render correctly on their first run.

## From the old code

The old code had it built, about 3,000 lines, and its approach can be reused.

Lessons:
- Bundling the image made installers about 440 MiB instead of 35 MiB; download on first use.
- The runtime's image load does not accept an outer gzip stream.
- Publish the guest image release before the app release that pins it, since the app downloads it directly.
- The image import lock is in-process only; a cross-process lock is missing.
- Build tools had to stream archives instead of loading them into memory.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/engine/src/guest_image.rs`
- `docs/SiloUI-GUEST-IMAGES.md#L443`
- `app/SiloUI/guest-image/image-lock.json`
- `app/SiloUI/src-tauri/engine/src/preparation.rs`
- `app/SiloUI/scripts/build-guest-image.mjs`
- `.github/workflows/guest-image.yml`
- `docs/SiloUI-GUEST-IMAGES.md`
- `app/SiloUI/api/src/methods/preparation.rs`
- `docs/SiloUI-PREPARATION.md`
- `app/SiloUI/src/desktop/preparation.ts`
- `app/SiloUI/src/features/application/components/preparation-toast.tsx`
- `app/SiloUI/src-tauri/src/commands/preparation.rs`

Calls: `preparation.status`, `preparation.retry`.
