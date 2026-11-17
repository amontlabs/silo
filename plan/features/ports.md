## Notes

- A reverse proxy in the engine routes by host name, so several forks can all serve port 3000 at once at `http://<fork>.silo.localhost:<port>`, with no DNS setup or administrator rights. Routes go away when the fork is dropped.

## From the old code

The old code had it built and driven end to end, about 4,600 lines, and its approach can be reused.

Lessons:
- Changing a published-site origin discards browser state, so the name-hash.localhost origins must not change.
- A fork inherits its source's published port and asks to publish it again (V I3a).
- `silo ports` inside a computer read network.state, which a computer may not call.
- The upstream publisher did not forward guest EOF, so HTTP/1.0 responses hung; the patch forwards half-closes both ways.
- Listening alone never marks a service reachable; a failed inspection is an error, never an empty list.
- Never bind guest port numbers on the host: a user's own later server on :3000 would fail.
- Safari resolves *.localhost only from macOS 26.
- Origins are stored in browsers: changing the naming scheme discards their state.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/network.rs`
- `app/SiloUI/src-tauri/engine/src/network.rs`
- `app/SiloUI/src-tauri/engine/src/url_proxy.rs`
- `docs/SiloUI-URL-PROXY.md`
- `docs/direction/evidence/V.txt`
- `app/SiloUI/cli/src/access.rs`
- `docs/direction/evidence/T5a.txt`
- `docs/SiloUI-NETWORK-PLAN.md#L140`
- `app/SiloUI/patches/microsandbox-live-public-ports-0.7.14.patch`
- `docs/SiloUI-MANAGED-SSH.md#L70`
- `docs/v1/evidence/phase-3/urls-check.txt`
- `app/SiloUI/src/features/application/pages/network-page.tsx`

Calls: `network.publish`, `network.unpublish`, `network.state`, `network.urls`, `network.open`, `silo ports`, `silo publish`, `silo unpublish`, `ports`, `publish`.
