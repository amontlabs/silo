## Proposed

- Property: Neither a person nor any agent can read a secret from a layer above their own.
- Secrets come in layers, where an admin gives fine-grained secrets to each person and a person adds their own.
- A person can lend a secret for one fork and one task, usable and never readable, for example to pay for someone else's inference.

## Notes

- The company hosts the proxy; Silo ships the software and never holds anyone's secrets.
- Read values from existing vaults (1Password, Doppler, HashiCorp Vault) rather than building one.
- Describe the mechanism only, never "secure".
- Shared computers carry secret names only, resolved to the recipient's own or the company's values.
