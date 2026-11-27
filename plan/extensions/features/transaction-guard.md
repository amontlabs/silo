# The transaction guard

polarzero, 27 November 2026:

> App I'm building makes it super easy to write extensions that sit at the network boundary it
> would be super cool to do some builtin crypto extensions. E.g. simulate crypto transactions with
> guillotine/tevm and hold outbound request for your review if you're getting rekt
>
> Or let your agents do anything without approvals inside the vm but it has to declare a set of
> transactions and hold/block them if your balance decreases
>
> [...] you can record a simulation from outside by catching it at the boundary, and yes maybe
> compare what actually comes back

## The choice

An agent's part, 27 November 2026. An agent manages a position for you. It asks to sign a
transaction that calls `approve` on a token for an address you have never used, with no limit.
Your balance does not move. A week later the address takes everything.

The options:

- A guard on the request for a signature (recommended). The key lives in a wallet computer
  (`blockchain-keys`), so every transaction and every signed message passes one request, and the
  guard is asked about it. It costs a wallet computer and the entries listed below, none of which
  exists yet.
- A guard on the node's host. The extension reads `eth_sendRawTransaction` on the hosts its
  manifest names. It needs no wallet computer. It costs every signature that never reaches a
  node, such as a permit, and it sees every transaction only on a computer limited to those hosts.
- A rule only. A rule holds every request for a signature, with no simulation. It executes
  nothing. It costs an ask for each one, with nothing to tell you what the transaction does.

Recommended: a guard on the request for a signature.

## Why it fits

An agent's part. A simulation can be wrong: the chain's state moves between the simulation and
the block, and a contract can behave differently when simulated. So its result is an opinion, and
an opinion may hold or deny and never allow (`less-sure-less-power`). The guard only holds. What
allows a transaction is the rule you set on the channel to the wallet computer, and your yes to a
declaration.

## What it needs that does not exist yet

- The body of a request to a channel its manifest names (`request-content-to-an-extension`).
- The response of the node, afterwards (`response-to-an-extension`).
- An explanation you can read in the ask (`explanation-in-an-ask`).
- A declaration you approve once (`declaration-is-a-report`).
- The key outside main (`blockchain-keys`).

## Two things it must get right

- "The balance decreases" is not the test. Fees lower it on every transaction, a swap lowers one
  token and raises another, and an approval changes no balance. The guard reports what leaves,
  what arrives and who is given the right to spend.
- A hold has a limit (`hold-limit`), and a person needs time to read a transaction. The limit for
  a hold by this extension is to find out.

## What it would look like

An agent's sketch. It was never run. The names of the interface are invented, since none is
written yet (`extension-tools`), and the call to tevm is from memory. It shows the length: if the
real one is much longer, the interface is wrong. It leaves out the comparison with the receipt.

```json
{ "name": "transaction-guard", "version": "0.1.0", "recipe": "recipe.silo",
  "rules": [{ "read": { "channel": "wallet" }, "as": "json-rpc" }],
  "tools": [
    { "reads": "requests", "to": "rules" },
    { "reads": "record", "kinds": ["declaration"] },
    { "protects": "requests", "to": "rules", "may": ["hold"] }],
  "wakes": [{ "on": "request", "operation": ["eth_sendTransaction", "eth_signTypedData_v4"],
              "run": "node guard.js", "limit_ms": 5000, "no_answer": "deny" }] }
```

```ts
import { wake, answer, read } from "@silo/extension";
import { createMemoryClient, http } from "tevm";
import { keccak256, toHex } from "viem";

const TRANSFER = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const APPROVAL = "0x8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925";
const w = await wake(), call = w.body, text: string[] = [], lines = [w.id];

if (call.method === "eth_signTypedData_v4") text.push("A signed message can move tokens with no transaction. Read it.");
else {
  const tx = call.params[0], key = keccak256(toHex(JSON.stringify([tx.to, tx.data ?? "0x", tx.value ?? "0x0"])));
  const fork = createMemoryClient({ fork: { transport: http(process.env.FORK_RPC_URL!) } });
  const sim = await fork.tevmCall({ from: tx.from, to: tx.to, data: tx.data, value: BigInt(tx.value ?? 0), throwOnFail: false });
  const mine = (topic: string) => (sim.logs ?? []).filter(l => l.topics[0] === topic && BigInt(l.topics[1]) === BigInt(tx.from)).length;
  const got = { ok: !sim.errors?.length, wei: String(BigInt(tx.value ?? 0)), transfers: mine(TRANSFER), approvals: mine(APPROVAL) };
  const [approved] = await read({ kinds: ["declaration"], where: { name: "eth-tx", key, answer: "yes" } });
  if (approved) lines.push(approved.id);
  const asDeclared = approved && got.ok && JSON.stringify(approved.body.expects) === JSON.stringify(got);
  if (!asDeclared) text.push(approved ? `You approved ${JSON.stringify(approved.body.expects)}; the simulation gives ${JSON.stringify(got)}.`
    : `Not approved by you. ${got.wei} wei, ${got.transfers} transfer(s) and ${got.approvals} approval(s) leave ${tx.from} for ${tx.to}.`);
}
answer(text.length ? { answer: "hold", explanation: { text: text.join(" "), lines } } : { answer: "go" });
```

Two things this sketch gets wrong on purpose, to stay short. It counts transfer and approval
logs, where a real guard compares balances and allowances before and after. And it starts a
program for each request: starting Node and fetching the chain's state will pass the 5 seconds
given, so a real one keeps a process running, or holds at once and simulates during the hold.
