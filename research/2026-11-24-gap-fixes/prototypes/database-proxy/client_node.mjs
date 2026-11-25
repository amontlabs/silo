// node-postgres through the boundary: SCRAM with the placeholder, the extended protocol
// (Parse/Bind/Execute), a held TRUNCATE, and a DELETE whose WHERE clause limits nothing.
// Run: NODE_PATH=<folder holding pg> DIAL=127.0.0.1 SILO_CA=<ca.pem> DATABASE_URL=... node client_node.mjs
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.NODE_PATH + "/");
const pg = require("pg");

// Here the name db.prod.example does not resolve, so the client dials DIAL and gives the name
// in the TLS handshake (SNI) and for the certificate check. In Silo the boundary answers the
// name lookup itself.
const u = new URL(process.env.DATABASE_URL);
const client = new pg.Client({
  user: decodeURIComponent(u.username), password: decodeURIComponent(u.password),
  database: u.pathname.slice(1), port: Number(u.port || 5432),
  host: process.env.DIAL || u.hostname,
  // SILO_CA set: trust only that CA. Unset: Node's own store plus NODE_EXTRA_CA_CERTS.
  ssl: { servername: u.hostname,
         ...(process.env.SILO_CA ? { ca: fs.readFileSync(process.env.SILO_CA) } : {}) },
});
await client.connect();
const r = await client.query("select current_user, count(*)::int as n from orders where total > $1", [100]);
console.log("read with a parameter (extended protocol):", r.rows[0]);

let t0 = Date.now();
try {
  await client.query({ text: "truncate orders", values: [], name: "t1" });
  console.log("TRUNCATE ran");
} catch (e) {
  console.log(`TRUNCATE refused after ${Date.now() - t0} ms: code=${e.code} message=${e.message} detail=${e.detail}`);
}

// A hostile agent writes a WHERE that limits nothing. Inside a transaction we roll back.
await client.query("begin");
await client.query({ text: "delete from orders where total > $1 or true", values: [0], name: "wipe" });
const gone = await client.query("select count(*)::int as n from orders");
console.log(`DELETE ... WHERE total > $1 OR true was let through; orders left inside the transaction: ${gone.rows[0].n}`);
await client.query("rollback");
const n = await client.query("select count(*)::int as n from orders");
console.log("orders after rollback:", n.rows[0].n);
await client.end();
