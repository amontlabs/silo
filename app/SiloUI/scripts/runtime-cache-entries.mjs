// The compiled runtime entries of this checkout's runtime cache, for the release workflows
// (.github/actions/prepare-release-runtime). Each command computes what `runtime:prepare`
// computes, with the same functions and the same environment:
//
//   tools --target <triple>         digest of the build tool identity and the cache format (parts of
//                                   the public cache key, read before the cache lookup)
//   keys --target <triple>          the msb and Git LFS build keys
//   keep-current --target <triple>  after preparation: checks that both entries of this build are
//                                   published and removes every other compiled entry and every
//                                   other MicroSandbox version, Git LFS pin and Git release, so a
//                                   saved or transferred cache holds this build's inputs only.
//                                   For CI, where each job owns its cache; elsewhere it refuses
//                                   without --yes-shared-cache-is-mine, because other checkouts
//                                   sharing the cache (SILO_RUNTIME_CACHE_DIR) may need what it
//                                   removes. It removes nothing while another process prepares
//                                   from the same cache (a live work folder in its `tmp`).
import { readdir, rm } from "node:fs/promises"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { RUNTIME_CACHE_FORMAT, cacheTemporaryRoot, otherLiveWork, runtimeCacheRoot, takePublishedEntry } from "./build-input.mjs"
import { DUGITE_RELEASE } from "./git-runtime.mjs"
import { LFS_TRANSFER_COMMIT, lfsTransferCache, readLfsBuildIdentity } from "./lfs-transfer-runtime.mjs"
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
  const busy = await otherLiveWork(cacheTemporaryRoot(cacheRoot))
  if (busy.length > 0) return { ...keys, removed: [], busy }
  const version = join(cacheRoot, `v${MICRO_SANDBOX_VERSION}`)
  const lfsPin = join(cacheRoot, "git-lfs-transfer", LFS_TRANSFER_COMMIT)
  const dugite = join(cacheRoot, "dugite", DUGITE_RELEASE)
  const removed = [
    ...(await folders(cacheRoot)).filter(path => /^v[^/]*$/.test(path.slice(cacheRoot.length + 1)) && path !== version),
    ...(await folders(join(version, "patched-builds"))).filter(build => build !== dirname(keys.msbEntry)),
    ...(await folders(join(cacheRoot, "git-lfs-transfer"))).filter(pin => pin !== lfsPin),
    ...(await folders(join(lfsPin, "builds"))).filter(build => build !== keys.lfsEntry),
    ...(await folders(join(cacheRoot, "dugite"))).filter(release => release !== dugite),
  ]
  for (const path of removed) await rm(path, { recursive: true, force: true })
  return { ...keys, removed, busy }
}

export const SHARED_CACHE_CONFIRMATION = "--yes-shared-cache-is-mine"

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, flag, targetTriple, ...options] = process.argv.slice(2)
  const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..")
  const confirmed = options.includes(SHARED_CACHE_CONFIRMATION)
  if (flag !== "--target" || !targetTriple || !["tools", "keys", "keep-current"].includes(command) ||
      options.some(option => option !== SHARED_CACHE_CONFIRMATION) || (confirmed && command !== "keep-current")) {
    console.error(`usage: runtime-cache-entries.mjs tools|keys --target <triple>\n       runtime-cache-entries.mjs keep-current --target <triple> [${SHARED_CACHE_CONFIRMATION}]`)
    process.exit(2)
  }
  if (command === "keep-current" && !confirmed && process.env.GITHUB_ACTIONS !== "true" && process.env.CI !== "true") {
    console.error(`keep-current removes every other build and input from the runtime cache (${runtimeCacheRoot(resolve(appRoot, "src-tauri"))}), which other checkouts sharing it may still use. It runs in CI, where each job owns its cache; to run it here, pass ${SHARED_CACHE_CONFIRMATION}.`)
    process.exit(2)
  }
  if (command === "tools") console.log(`digest=${await buildToolDigest({ appRoot, targetTriple })}\nformat=${RUNTIME_CACHE_FORMAT}`)
  else {
    const result = command === "keys" ? await currentBuildKeys({ appRoot, targetTriple }) : await keepCurrentEntries({ appRoot, targetTriple })
    console.log(`msb=${result.msb}\nlfs=${result.lfs}`)
    if (result.busy?.length) console.error(`Another process is preparing from this runtime cache, so nothing was removed: ${result.busy.join(", ")}`)
    for (const path of result.removed ?? []) console.error(`Removed an older cache entry: ${path}`)
  }
}
