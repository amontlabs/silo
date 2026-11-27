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

An agent's part follows.

## Why it fits

A simulation can be wrong: the chain's state moves between the simulation and the block, and a
contract can behave differently when simulated. So its result is an opinion, and an opinion may
hold or deny and never allow (`less-sure-less-power`). That is exactly what the guard does. A
transaction is also the plainest case of what Silo's checkpoints cannot bring back.

## What it needs that does not exist yet

- The body of a request to a host its manifest names (`request-content-to-an-extension`).
- The response, afterwards (`response-to-an-extension`).
- A finding you can read in the ask (`finding-of-an-extension`).
- A declaration you approve once (`declaration-is-a-report`).
- The key outside the computer (`signing-outside-the-computer`). With the key inside, a signed
  transaction can leave by any host that is not read, and a signature that moves tokens with no
  transaction, such as a permit, never reaches a node.

## Three things it must get right

- "The balance decreases" is not the test. Fees lower it on every transaction, a swap lowers one
  token and raises another, and an approval changes no balance and lets someone take everything
  later. The guard reports what leaves, what arrives and who is given the right to spend.
- It is exact only on a computer limited to the hosts it reads. Its manifest says so, and
  installing it on a computer with an open network is refused.
- A hold has a limit (`hold-limit`), and a person needs time to read a transaction. The limit for
  a hold by this extension is to find out.

## What it would look like

An agent's sketch. It was never run. The names of the interface are invented, since none is
written yet (`extension-tools`), and the calls to tevm and viem are from memory. It is here to
show the length: if the real one is much longer, the interface is wrong.

```json
{ "name": "transaction-guard", "version": "0.1.0", "recipe": "recipe.silo",
  "rules": [{ "read": ["mainnet.infura.io", "eth-mainnet.g.alchemy.com"], "as": "json-rpc" }],
  "tools": [
    { "reads": "requests", "hosts": "rules" },
    { "reads": "record", "kinds": ["declaration"] },
    { "protects": "requests", "hosts": "rules", "may": ["hold"] },
    { "shows": "alert" }],
  "wakes": [
    { "on": "request", "operation": "eth_sendRawTransaction", "run": "node guard.js",
      "limit_ms": 5000, "no_answer": "deny" },
    { "on": "lines", "kind": "connection", "operation": "eth_getTransactionReceipt",
      "run": "node guard.js", "limit_ms": 10000 }] }
```

```ts
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { wake, answer, read, alert } from "@silo/extension";
import { createMemoryClient, http } from "tevm";
import { keccak256, parseTransaction, recoverTransactionAddress } from "viem";

const TRANSFER = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const sent = (logs: any[], from: string) =>
  logs.filter(l => l.topics[0] === TRANSFER && BigInt(l.topics[1]) === BigInt(from)).length;
const w = await wake();

if (w.wake === "request") {
  const text: string[] = [], lines = [w.id];
  for (const call of [w.body].flat().filter(c => c.method === "eth_sendRawTransaction")) {
    const raw = call.params[0], tx = parseTransaction(raw), hash = keccak256(raw);
    const from = await recoverTransactionAddress({ serializedTransaction: raw });
    const fork = createMemoryClient({ fork: { transport: http(process.env.FORK_RPC_URL!) } });
    const sim = await fork.tevmCall({ from, to: tx.to!, data: tx.data, value: tx.value, throwOnFail: false });
    const got = { ok: !sim.errors?.length, value: String(tx.value ?? 0n), tokensOut: sent(sim.logs ?? [], from) };
    writeFileSync(`/state/${hash}.json`, JSON.stringify(got));
    const [approved] = await read({ kinds: ["declaration"], where: { name: "eth-tx", key: hash, answer: "yes" } });
    if (approved) lines.push(approved.id);
    else text.push(`You did not approve ${hash}.`);
    if (tx.value || got.tokensOut) text.push(`${got.value} wei and ${got.tokensOut} token transfer(s) leave ${from} for ${tx.to}.`);
    if (approved && got.ok) text.length = 0;      // approved by you and it simulates as declared
  }
  answer(text.length ? { answer: "hold", finding: { text: text.join(" "), lines } } : { answer: "go" });
}

if (w.wake === "lines") for (const line of w.lines)
  for (const { result: r } of [line.response?.body ?? []].flat()) {
    const file = `/state/${r?.transactionHash}.json`;
    if (!r?.logs || !existsSync(file)) continue;
    const sim = JSON.parse(readFileSync(file, "utf8")), out = sent(r.logs, r.from);
    if ((r.status === "0x1") !== sim.ok || out !== sim.tokensOut)
      await alert({ text: `Simulated ${JSON.stringify(sim)}; the chain gave status ${r.status}, ${out} transfer(s).`, lines: [line.id] });
  }
```

Two things this sketch gets wrong on purpose, to stay short. It counts transfer logs where a real
guard compares balances and allowances before and after. And it starts a program for each
request: starting Node and fetching the chain's state will pass the 5 seconds given, so a real one
keeps a process running or holds at once and simulates during the hold.
