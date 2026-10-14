# Editor micro-review

Scope: `app/SiloUI/src-tauri/src/editor.rs`.

Read-only source review. No cargo/npm commands, builds, executable reproductions, app launches, or live data access. Checked the first, second, and third review-pass reports; omitted the previously reported repair-error finding R-36.

## EDITOR-1: Reopening a commented workspace deletes user configuration

- **Priority:** P2.
- **Location:** `app/SiloUI/src-tauri/src/editor.rs:251–264`.
- **Trigger:** A user adds a supported JSON comment to the Silo-created `.code-workspace` file, alongside custom settings, tasks, launch configuration, or extension recommendations, then opens the same sandbox folder through Silo again.
- **Evidence:** `vscode_workspace` parses the existing document with strict `serde_json::from_slice`, discards the parse error with `.ok()`, substitutes `{}`, and persists that replacement at the same path. A `//` comment makes strict JSON parsing fail. VS Code explicitly supports comments in workspace files, as documented in its [workspace file schema](https://code.visualstudio.com/docs/editing/workspaces/multi-root-workspaces#_workspace-file-schema). The existing preservation test at lines 1298–1317 supplies only strict JSON, so it does not cover this valid editor document format.
- **Consequence:** The next handoff irreversibly replaces the user's workspace configuration with the folder, remote authority, and four Silo settings. No error or backup preserves the discarded content.
- **Suggested fix:** Read and update the supported JSON-with-comments format using a maintained parser. Preserve unrelated fields. On an existing document that cannot be parsed, return an actionable error and leave its bytes intact instead of treating it as a new workspace.
- **Test that would catch it:** Create the workspace through the production helper, replace its contents with a valid commented workspace containing a custom setting, task, and extension recommendation, then reopen it. Assert those values survive while Silo's settings are restored. Separately assert malformed existing content is not overwritten.

## EDITOR-2: Literal percent sequences change the editor's destination folder

- **Priority:** P2.
- **Location:** `app/SiloUI/src-tauri/src/editor.rs:268–276`.
- **Trigger:** Open an existing guest folder named `/workspace/some%20comments`. `validate_path` accepts this literal filesystem path and the guest directory check receives those exact bytes.
- **Evidence:** `remote_uri` passes the raw guest path to `Url::set_path`. The pinned `url` version is 2.5.8 (`app/SiloUI/Cargo.lock`); its local source documents that `set_path("api/some%20comments")` preserves `%20` rather than escaping the percent sign (`url-2.5.8/src/lib.rs:1757–1762`). The same behavior appears in the [upstream API documentation](https://docs.rs/url/2.5.8/url/struct.Url.html#method.set_path). Therefore the emitted URI has `/workspace/some%20comments`, rather than the required `/workspace/some%2520comments`. The same helper supplies both the VS Code workspace folder URI and the Zed SSH URI. The existing encoding test at lines 1117–1125 covers spaces, `#`, and `?`, but no literal percent sequence.
- **Consequence:** URI decoding addresses `/workspace/some comments`, which differs from the folder Silo validated. The editor opens the wrong folder if it exists, or fails to open the requested folder. This is a source-confirmed serialization mismatch; no live editor handoff was executed.
- **Suggested fix:** Serialize raw filesystem path segments through `path_segments_mut` so literal percent signs are encoded, while preserving separator boundaries. Do not pass unencoded filesystem paths to a setter that accepts existing URI escapes.
- **Test that would catch it:** For both URI schemes, serialize `/workspace/some%20comments` and assert the URI contains `some%2520comments`; decode it through the editor's URI parser and assert the original literal path is recovered. Also cover literal `%2F` and `%2e%2e` components so URI syntax cannot change path boundaries or normalize away a filesystem component.
