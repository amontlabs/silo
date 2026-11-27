# Signing outside the computer

## The choice

An agent's part, 27 November 2026. An agent trades from an Ethereum account. If the private key
is a file in main, an extension that reads the node's host protects nothing: the agent signs
inside and sends the signed transaction by any host the rules allow. A signature that moves
tokens without a transaction, such as a permit, never reaches a node at all. So the place to
check is the signature, and the key has to be outside.

`secret-placeholder` leaves keys that sign "to find out". `ssh-keys` answers for SSH: the key
stays on your Mac and the computer can only ask for a signature. `signing-keys-re-signed` answers
for App Store Connect and AWS: the computer signs with a placeholder key and the boundary signs
again with the real one. A blockchain key fits the first and not the second. An account's address
is derived from its key, so a placeholder key is another account, with another balance.

The known practice is an external signer: the program sends the transaction unsigned, and a
separate process that holds the key applies rules, asks a person when they say so, and signs. A
hardware wallet and Clef, the signer of go-ethereum, both work this way.

The options:

- One rule for every key that signs, with the wallet as a computer (recommended). The key lives
  in an ordinary computer that main reaches through a channel. Main's request to it crosses main's
  boundary, so a rule allows it and `transaction-guard` may hold it first. Signing is granting,
  and here a rule you set does it, never an extension (`no-tool-grants-or-destroys`). It costs one
  more running computer, and a wallet computer to write.
- The boundary signs transactions itself. It costs code for one kind of chain inside Silo's core:
  transaction types, the curve, nonces, fees.
- The key stays in main. It costs the whole check.

Recommended: one rule for every key that signs, with the wallet as a computer. The second option
stays open as a later ruling of polarzero's.
