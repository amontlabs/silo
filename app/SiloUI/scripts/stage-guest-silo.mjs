// Builds the Linux `silo` for the host's architecture and stages it where the app bundles it
// (src-tauri/runtime/guest-silo). Prints the program's path, which `silo tools enable --binary`
// takes as a development override. Needs Zig (docs/SiloUI-RUNTIME-PACKAGING.md).
import { execFileSync } from "node:child_process"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"

import { stageGuestSilo } from "./guest-silo-runtime.mjs"
import { resolveRuntimeTarget } from "./microsandbox-runtime.mjs"

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..")
const hostTriple = execFileSync("rustc", ["--print", "host-tuple"], { encoding: "utf8" }).trim()
const targetTriple = resolveRuntimeTarget(process.env, () => hostTriple)
const staged = await stageGuestSilo({ appRoot, targetTriple, log: message => console.error(message) })
if (!staged.available) {
  console.error(staged.reason)
  process.exit(1)
}
console.log(staged.binaryPath)
