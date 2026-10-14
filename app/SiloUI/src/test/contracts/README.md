# Native wire contracts

These JSON files describe the payloads accepted by the frontend Zod parsers.
Rust tests serialize native structs or execute the native state read path and
compare the emitted JSON with the checked-in files. Frontend tests parse the
same files and verify that the fields needed by the UI survive parsing.

The application fixtures run `read_application_state_with` with a deterministic
runtime runner and temporary metadata, checkpoint, lifecycle, and secret stores.
The test fixes the measured device capacity before serialization because the
device running the test changes that value. The remote snapshot fixtures apply
the same `ApplicationSource` to JSON conversion as `runtime.snapshot` dispatch;
`device_snapshot` passes that payload through. They cover the wire contract,
not SSH transport or live computer health.

SSH access, network state, and operation queue fixtures serialize the native
response structs. They exercise all currently supported listener states, port
states, and operation kinds, including null fields and optional fields. The
network fixture also uses the production `pending` and `computer_host` functions.
They do not start listeners, create forwards, or admit live operations.

To regenerate these five fixture files after an intentional native wire change,
run this command from the repository root:

```sh
nice -n 10 env \
  SILO_GITHUB_APP_SLUG=silo-test \
  SILO_GITHUB_CLIENT_ID=test-client \
  SILO_GITHUB_CLIENT_SECRET=test-secret \
  SILO_UPDATE_CONTRACT_FIXTURES=1 \
  cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml --locked \
  contract_tests
```

Review the JSON diff and update the frontend parser when needed. Repeat the Rust
command without `SILO_UPDATE_CONTRACT_FIXTURES`, then run from `app/SiloUI/`:

```sh
nice -n 10 npx vitest run --testTimeout=30000 src/desktop/native-contracts.test.ts
```

The Rust test executables use synthetic GitHub configuration and must not be
distributed. Normal tests never rewrite fixtures. Existing GitHub, backup, and
setup activity fixtures retain their dedicated Rust verification tests. To regenerate
export/import operation wording, use the same synthetic configuration and update
flag with the `backup_operation_serialization_matches_frontend_contract` filter,
repeat without the update flag, then run `src/desktop/production-source.test.ts`.

`github-authentication.json` exercises native policies with a null, OAuth, or token
method. [Native policy validation](../../../src-tauri/engine/src/github.rs) accepts null
as the default OAuth method and returns the saved value; the
[frontend parser](../../desktop/production-source.ts) normalizes it to its optional
authentication field. Regenerate it with
the same synthetic configuration and update flag using the
`nullable_github_authentication_matches_wire_contract` filter, then run
`src/desktop/native-contracts.test.ts` without rewriting the fixture.
