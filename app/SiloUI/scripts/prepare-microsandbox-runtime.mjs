import { readFileSync, writeFileSync } from "node:fs"
import { execFileSync } from "node:child_process"
import { fileURLToPath } from "node:url"
import { dirname, resolve } from "node:path"

import { resolveRuntimeTarget, stageRuntime } from "./microsandbox-runtime.mjs"
import { stageLfsTransferRuntime } from "./lfs-transfer-runtime.mjs"
import { stageGitRuntime } from "./git-runtime.mjs"
import { preflight } from "./preflight.mjs"
import { fetchStream } from "./build-input.mjs"
import { stageLinuxPackageTools } from "./linux-package-tools.mjs"

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..")
preflight(appRoot)
const hostTriple = execFileSync("rustc", ["--print", "host-tuple"], { encoding: "utf8" }).trim()
const targetTriple = resolveRuntimeTarget(process.env, () => hostTriple)

const prepared = await stageRuntime({
  appRoot,
  targetTriple,
  hostTriple,
  fetchStream,
})

const git = await stageGitRuntime({
  appRoot,
  targetTriple,
  fetchStream,
})

await stageLfsTransferRuntime({ appRoot, targetTriple, fetchStream })

console.log(`Prepared bundled MicroSandbox ${prepared.targetTriple}`)
console.log(`Prepared bundled Git ${git.targetTriple}`)

// Signed package metadata lets publication verify version and target without running it.
const { version } = JSON.parse(readFileSync(resolve(appRoot, "package.json"), "utf8"))
writeFileSync(resolve(appRoot, "src-tauri/runtime/release-info.json"), JSON.stringify({ version, target: targetTriple }) + "\n")

if (targetTriple.endsWith("-unknown-linux-gnu")) {
  const packageTools = await stageLinuxPackageTools({ appRoot, targetTriple })
  console.log(`Prepared Linux package tools ${packageTools.targetTriple}`)
}
