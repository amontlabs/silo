// The compiled runtime entries of this checkout's runtime cache, for the release workflows
// (.github/actions/prepare-release-runtime). Each command computes what `runtime:prepare`
// computes, with the same functions and the same environment:
//
//   tools --target <triple>         digest of the build tool identity (part of the public cache key,
//                                   read before the cache lookup)
//   keys --target <triple>          the msb and Git LFS build keys
//   keep-current --target <triple>  after preparation: checks that both entries of this build are
//                                   published and removes every other compiled entry, so a saved
//                                   or transferred cache holds this build only
import { readdir, rm } from "node:fs/promises"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { cacheTemporaryRoot, runtimeCacheRoot, takePublishedEntry } from "./build-input.mjs"
import { lfsTransferCache, readLfsBuildIdentity } from "./lfs-transfer-runtime.mjs"
import { MICRO_SANDBOX_VERSION, patchedBuildEntry, patchedBuildInputs, readBuildToolIdentity, selectRuntime, sha256 } from "./microsandbox-runtime.mjs"

export async function buildToolDigest({ appRoot, targetTriple, environment = process.env }) {
  const cacheRoot = runtimeCacheRoot(resolve(appRoot, "src-tauri"), environment)
  const tools = await readBuildToolIdentity({ cargoDirectory: cacheTemporaryRoot(cacheRoot), targetTriple, environment })
  return sha256(Buffer.from(JSON.stringify(tools))).slice(0, 16)
}

export async function currentBuildKeys({ appRoot, targetTriple }) {
  const tauriRoot = resolve(appRoot, "src-tauri")
  const cacheRoot = runtimeCacheRoot(tauriRoot)
  const msb = await patchedBuildInputs({ appRoot, targetTriple, agentdSha256: selectRuntime(targetTriple).agentdSha256, temporaryRoot: cacheTemporaryRoot(cacheRoot) })
  const lfs = await readLfsBuildIdentity({ appRoot, targetTriple })
  return {
    msb: msb.cacheKey,
    msbEntry: patchedBuildEntry(join(cacheRoot, `v${MICRO_SANDBOX_VERSION}`), msb.cacheKey),
    msbNames: msb.gatewayInputs ? ["msb", "silo-macos-gateway"] : ["msb"],
    lfs: lfs.buildKey,
    lfsEntry: join(lfsTransferCache(tauriRoot), "builds", lfs.buildKey),
  }
}

async function folders(directory) {
  try {
    return (await readdir(directory, { withFileTypes: true })).filter(entry => entry.isDirectory()).map(entry => join(directory, entry.name))
  } catch (error) {
    if (error.code === "ENOENT") return []
    throw error
  }
}

export async function keepCurrentEntries({ appRoot, targetTriple }) {
  const keys = await currentBuildKeys({ appRoot, targetTriple })
  if (!await takePublishedEntry(keys.msbEntry, keys.msbNames)) {
    throw new Error(`No published MicroSandbox build for key ${keys.msb}: this step and runtime:prepare computed different keys`)
  }
  if (!await takePublishedEntry(keys.lfsEntry, null)) {
    throw new Error(`No published Git LFS transfer build for key ${keys.lfs}: this step and runtime:prepare computed different keys`)
  }
  const cacheRoot = runtimeCacheRoot(resolve(appRoot, "src-tauri"))
  const removed = []
  for (const version of (await folders(cacheRoot)).filter(path => /^v[^/]*$/.test(path.slice(cacheRoot.length + 1)))) {
    for (const build of await folders(join(version, "patched-builds"))) {
      if (build !== dirname(keys.msbEntry)) removed.push(build)
    }
  }
  for (const pin of await folders(join(cacheRoot, "git-lfs-transfer"))) {
    for (const build of await folders(join(pin, "builds"))) {
      if (build !== keys.lfsEntry) removed.push(build)
    }
  }
  for (const path of removed) await rm(path, { recursive: true, force: true })
  return { ...keys, removed }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, flag, targetTriple] = process.argv.slice(2)
  const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..")
  if (flag !== "--target" || !targetTriple || !["tools", "keys", "keep-current"].includes(command)) {
    console.error("usage: runtime-cache-entries.mjs tools|keys|keep-current --target <triple>")
    process.exit(2)
  }
  if (command === "tools") console.log(`digest=${await buildToolDigest({ appRoot, targetTriple })}`)
  else {
    const result = command === "keys" ? await currentBuildKeys({ appRoot, targetTriple }) : await keepCurrentEntries({ appRoot, targetTriple })
    console.log(`msb=${result.msb}\nlfs=${result.lfs}`)
    for (const path of result.removed ?? []) console.error(`Removed an older compiled entry: ${path}`)
  }
}
