# A signed receipt

## The choice

An agent's part, 24 November 2026. A developer pastes a receipt into a pull request. It says the
tests passed and that no host outside the allowed list was reached. The reviewer, or an auditor a
year later, cannot tell whether the developer edited it.

The known practice is a signed statement about an artefact, as in-toto and Sigstore do for builds.
A signature says who made a statement and that it was not changed. It does not say that the maker
was honest.

The options:

- Signed by the Silo that made it (recommended). Each Silo has its own key, kept in the Mac's
  Keychain. The receipt carries a signature over its content, and `silo` checks it. It costs
  managing one key per Mac, and the honesty of saying what it does not prove: the member controls
  their Mac and could run a modified Silo.
- Unsigned. It costs any use of a receipt as evidence.
- Signed by a service of Amont Labs. It costs a server, and sending each receipt home.

Recommended: signed by the Silo that made it, with the limit written on the receipt.
