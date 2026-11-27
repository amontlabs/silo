# Where the shape rules fire on clean text

5122 text files, 40.1 MB, at origin/main, legacy-2026-11-16.

| Rule | Hits without the gate | Hits | Per MB | Files |
|---|---|---|---|---|
| assigned-secret | 765 | 24 | 0.60 | 17 |
| private-key | 20 | 20 | 0.50 | 5 |
| url-password | 19 | 3 | 0.07 | 3 |
| basic-authorization | 5 | 2 | 0.05 | 1 |
| aws-access-key-id | 3 | 3 | 0.07 | 2 |
| gitlab | 1 | 1 | 0.02 | 1 |
| slack | 1 | 1 | 0.02 | 1 |

- aws-access-key-id: legacy-2026-11-16:app/SiloUI/cli/src/hosted/codediff.rs, legacy-2026-11-16:app/SiloUI/cli/tests/hosted.rs
- gitlab: legacy-2026-11-16:app/SiloUI/cli/src/hosted/codediff.rs
- slack: legacy-2026-11-16:app/SiloUI/cli/src/hosted/codediff.rs
- url-password: legacy-2026-11-16:app/SiloUI/cli/src/hosted/codediff.rs, legacy-2026-11-16:app/SiloUI/src-tauri/engine/src/runtime_activity.rs, legacy-2026-11-16:app/SiloUI/src-tauri/engine/src/runtime_logs.rs
- assigned-secret: legacy-2026-11-16:app/SiloUI/cli/src/hosted/codediff.rs, legacy-2026-11-16:app/SiloUI/cli/src/hosts.rs, legacy-2026-11-16:app/SiloUI/guest-image/image-lock.json, legacy-2026-11-16:app/SiloUI/scripts/test_release_notarization.py, legacy-2026-11-16:app/SiloUI/src-tauri/engine/src/guest/exec.rs, legacy-2026-11-16:app/SiloUI/src-tauri/engine/src/host_push.rs …
- basic-authorization: legacy-2026-11-16:app/SiloUI/src-tauri/engine/src/desktop_proxy.rs
- private-key: legacy-2026-11-16:app/SiloUI/src-tauri/engine/src/macos_computers/setup_log.rs, legacy-2026-11-16:app/SiloUI/src-tauri/engine/src/runtime_activity.rs, legacy-2026-11-16:app/SiloUI/src-tauri/vendor/russh/src/helpers.rs, legacy-2026-11-16:app/SiloUI/src-tauri/vendor/russh/src/keys/format/tests.rs, legacy-2026-11-16:app/SiloUI/src-tauri/vendor/russh/src/keys/mod.rs
