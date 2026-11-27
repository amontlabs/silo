# Blockchain keys

## The choice

An agent's part, 27 November 2026. An agent trades from an Ethereum account. If the private key
is a file in main, an extension that reads the node's host sees only what the agent chooses to
send there: the agent signs inside and sends the signed transaction by any host the rules allow.
A signature that moves tokens with no transaction, such as a permit, never reaches a node. So the
place to compare and to hold is the request for a signature, and the key has to be outside main.

`secret-placeholder` leaves keys that sign "to find out". Two proposals answer for other keys.
`ssh-keys`: the key stays on your Mac and the computer can only ask for a signature.
`signing-keys-re-signed`: the computer signs with a placeholder key and the boundary signs again
with the real one. A blockchain key can follow the first and not the second. An account's address
is derived from its key, so a placeholder key is another account, with another balance.

The known practice is an external signer: the program sends the transaction unsigned, and a
separate process that holds the key applies rules, asks a person when they say so, and signs. A
hardware wallet and Clef, the signer of go-ethereum, work this way.

The options:

- A wallet computer (recommended). Signing is granting. Here the rule you set on the channel
  grants it, and no extension does (`no-tool-grants-or-destroys`): the wallet computer has no tool
  and reads nothing of main. Its recipe can ship with Silo (`recipes-shipped-with-silo`). It costs
  one more running computer, and a wallet program to write.
- The boundary signs transactions itself. It costs code for one kind of chain inside Silo's core:
  transaction types, the curve, nonces, fees.
- The key stays in main. It costs the comparison and the hold.

Recommended: a wallet computer. The second option stays open as a later ruling of polarzero's.
