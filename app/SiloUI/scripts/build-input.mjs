import { spawnSync } from "node:child_process"
import { createHash } from "node:crypto"
import { createReadStream, createWriteStream, readFileSync, statSync } from "node:fs"
import { lstat, mkdir, mkdtemp, readdir, readFile, rename, rm, stat, writeFile } from "node:fs/promises"
import { basename, dirname, isAbsolute, join, resolve } from "node:path"
import { Readable, Transform, Writable } from "node:stream"
import { pipeline } from "node:stream/promises"

const MAX_INPUT_BYTES = 1024 * 1024 * 1024
const INPUT_TIMEOUT_MS = 10 * 60 * 1000

function verifier(expectedSha256, label, { bytes, maxBytes = bytes ?? MAX_INPUT_BYTES } = {}) {
  if (!Number.isSafeInteger(maxBytes) || maxBytes < 1 || (bytes !== undefined && (!Number.isSafeInteger(bytes) || bytes < 1 || bytes > maxBytes))) {
    throw new Error(`Invalid ${label} size limit`)
  }
  const hash = createHash("sha256")
  let length = 0
  return new Transform({
    transform(chunk, encoding, callback) {
      length += chunk.length
      if (length > maxBytes) return callback(Object.assign(new Error(`${label} exceeds its ${maxBytes}-byte size limit`), { code: "ERR_BUILD_INPUT_SIZE" }))
      hash.update(chunk)
      callback(null, chunk)
    },
    flush(callback) {
      const actual = hash.digest("hex")
      if ((bytes !== undefined && length !== bytes) || actual !== expectedSha256) {
        return callback(Object.assign(new Error(`${label} checksum mismatch: expected ${expectedSha256}, received ${actual} (${length} bytes)`), { code: "ERR_BUILD_INPUT_CHECKSUM" }))
      }
      callback()
    },
  })
}

export async function fetchStream(url, { signal }) {
  const response = await fetch(url, { redirect: "follow", signal })
  if (!response.ok || !response.body) {
    await response.body?.cancel()
    throw new Error(`Download failed (${response.status}) for ${url}`)
  }
  return Readable.fromWeb(response.body)
}

export async function verifyFile(path, expectedSha256, label, limits = {}) {
  const information = await stat(path)
  const maxBytes = limits.maxBytes ?? limits.bytes ?? MAX_INPUT_BYTES
  if (!information.isFile() || information.size > maxBytes) throw new Error(`${label} exceeds its ${maxBytes}-byte size limit or is not a file`)
  await pipeline(
    createReadStream(path, { highWaterMark: 1024 * 1024 }),
    verifier(expectedSha256, label, limits),
    new Writable({ write(chunk, encoding, callback) { callback() } }),
    { signal: AbortSignal.timeout(limits.timeoutMs ?? INPUT_TIMEOUT_MS) },
  )
}

export async function isVerifiedFile(path, expectedSha256, label, limits = {}) {
  try {
    await verifyFile(path, expectedSha256, label, limits)
    return true
  } catch {
    return false
  }
}

// `limits.temporaryRoot` (a cache root's `tmp`, see makeWorkFolder) holds the download until it is
// verified; without it the download is written beside `path`.
export async function writeVerifiedFile(path, openSource, expectedSha256, label, limits = {}) {
  const verification = verifier(expectedSha256, label, limits)
  const signal = AbortSignal.timeout(limits.timeoutMs ?? INPUT_TIMEOUT_MS)
  await mkdir(dirname(path), { recursive: true })
  const temporaryRoot = limits.temporaryRoot
    ? await makeWorkFolder(limits.temporaryRoot)
    : await mkdtemp(join(dirname(path), `.${basename(path)}.tmp-`))
  const temporary = join(temporaryRoot, "input")
  try {
    await pipeline(
      await openSource(signal),
      verification,
      createWriteStream(temporary, { flags: "wx", mode: 0o644 }),
      { signal },
    )
    signal.throwIfAborted()
    await rename(temporary, path)
  } finally {
    await rm(temporaryRoot, { recursive: true, force: true })
  }
  return path
}

export async function fetchVerifiedFile(fetchInput, url, expectedSha256, label, path, limits = {}) {
  if (await isVerifiedFile(path, expectedSha256, label, limits)) return path
  return writeVerifiedFile(path, signal => fetchInput(url, { signal }), expectedSha256, label, limits)
}

// Version of the runtime cache's format: the entry layout and manifest of `publishEntry` and
// `takePublishedEntry`, and what `runtime-cache-entries.mjs keep-current` keeps. Bump it when
// either changes; it is part of every compiled build key and of CI's cache key, so a format
// change misses instead of hitting an entry it then rejects.
export const RUNTIME_CACHE_FORMAT = 1

const skippedForeign = new Set()
// Notes once that `path` belongs to another user and is left alone.
function noteForeign(path, consequence) {
  if (skippedForeign.has(path)) return
  skippedForeign.add(path)
  console.warn(`${path} belongs to another user; ${consequence}`)
}

// Whether `path` exists and belongs to a user other than this process's.
function foreignOwned(information) {
  return information !== null && typeof process.getuid === "function" && information.uid !== process.getuid()
}

// Where preparation keeps downloads and content-keyed builds: by default in the checkout, so it
// belongs to the user building that checkout. SILO_RUNTIME_CACHE_DIR (absolute) lets every
// checkout of a machine share one root; a root that belongs to another user is not used, and the
// checkout's own is used instead.
export function runtimeCacheRoot(tauriRoot, environment = process.env) {
  const own = join(tauriRoot, "target", "runtime-cache")
  const shared = environment.SILO_RUNTIME_CACHE_DIR?.trim()
  if (!shared) return own
  if (!isAbsolute(shared)) throw new Error("SILO_RUNTIME_CACHE_DIR must be an absolute path")
  const root = resolve(shared)
  let information = null
  try {
    information = statSync(root)
  } catch (error) {
    if (error.code !== "ENOENT") throw error
  }
  if (foreignOwned(information)) {
    noteForeign(root, `SILO_RUNTIME_CACHE_DIR is skipped and this checkout's own cache ${own} is used`)
    return own
  }
  return root
}

// The folder of a cache root that holds work in progress: extracted sources, cargo targets,
// downloads and entries being published.
export function cacheTemporaryRoot(cacheRoot) {
  return join(cacheRoot, "tmp")
}

let bootId
// A process is named by its pid and the hash of its start time, so a reused pid is a different
// owner. The start time must read the same from every process, whatever its time zone and after
// the clock is set: on Linux it is the start in clock ticks since boot (field 22 of
// /proc/<pid>/stat) with the boot id; elsewhere it is `/bin/ps -o lstart=` in UTC (the start the
// kernel recorded at fork, printed in the time zone of `ps`). Null when there is no such process.
function processStart(pid) {
  let start
  if (process.platform === "linux") {
    let line
    try {
      line = readFileSync(`/proc/${pid}/stat`, "utf8")
    } catch {
      return null
    }
    // The command name (field 2) is in parentheses and may contain spaces and parentheses.
    const ticks = line.slice(line.lastIndexOf(")") + 2).split(" ")[19]
    if (!/^\d+$/.test(ticks ?? "")) return null
    bootId ??= readFileSync("/proc/sys/kernel/random/boot_id", "utf8").trim()
    start = `${bootId} ${ticks}`
  } else {
    const result = spawnSync("/bin/ps", ["-o", "lstart=", "-p", String(pid)], { encoding: "utf8", env: { ...process.env, LC_ALL: "C", TZ: "UTC0" } })
    if (result.error) throw result.error
    start = result.status === 0 ? result.stdout.trim() : ""
  }
  return start ? createHash("sha256").update(start).digest("hex").slice(0, 12) : null
}

let ownTag
// `<pid>-<start>`: names a temporary folder or file for the process that owns it.
export function ownerTag() {
  if (!ownTag) {
    const start = processStart(process.pid)
    if (!start) throw new Error("Cannot read this process's start time")
    ownTag = `${process.pid}-${start}`
  }
  return ownTag
}

const OWNED_NAME = /^(\d+)-([0-9a-f]{12})(?:$|[-.])/

async function lstatOrNull(path) {
  try {
    return await lstat(path)
  } catch (error) {
    if (error.code === "ENOENT") return null
    throw error
  }
}

async function names(directory) {
  try {
    return await readdir(directory)
  } catch (error) {
    if (error.code === "ENOENT") return []
    throw error
  }
}

// Removes the entries of `directory` named `<prefix><pid>-<start>…` whose owning process has
// exited, so work an interrupted run left behind is removed by the next run. With `prefix` "", an
// entry that is not named for an owner is removed too: such a folder holds only owned work. An
// entry of another user is left alone: this process can neither see its owner nor remove it.
export async function removeAbandoned(directory, prefix = "") {
  for (const name of await names(directory)) {
    if (!name.startsWith(prefix)) continue
    const owner = OWNED_NAME.exec(name.slice(prefix.length))
    if (!owner && prefix) continue
    if (owner && `${owner[1]}-${owner[2]}` === ownerTag()) continue
    if (owner && processStart(Number(owner[1])) === owner[2]) continue
    const path = join(directory, name)
    if (foreignOwned(await lstatOrNull(path))) {
      noteForeign(path, "it is left in place")
      continue
    }
    try {
      await rm(path, { recursive: true, force: true })
    } catch (error) {
      if (error.code !== "EACCES" && error.code !== "EPERM") throw error
      console.warn(`${path} cannot be removed (${error.code}); it is left in place`)
    }
  }
}

// The work folders of `temporaryRoot` (a cache root's `tmp`) that another running process owns,
// or that belong to another user (whose owner this process cannot check).
export async function otherLiveWork(temporaryRoot) {
  const live = []
  for (const name of await names(temporaryRoot)) {
    const owner = OWNED_NAME.exec(name)
    if (owner && `${owner[1]}-${owner[2]}` === ownerTag()) continue
    const path = join(temporaryRoot, name)
    if ((owner && processStart(Number(owner[1])) === owner[2]) || foreignOwned(await lstatOrNull(path))) live.push(path)
  }
  return live
}

// A new private folder in `temporaryRoot` (a cache root's `tmp`), after removing the folders of
// processes that have exited. The caller removes it when done.
export async function makeWorkFolder(temporaryRoot) {
  await mkdir(temporaryRoot, { recursive: true })
  await removeAbandoned(temporaryRoot)
  return mkdtemp(join(temporaryRoot, `${ownerTag()}-`))
}

const ENTRY_MANIFEST = "manifest.json"
const ENTRY_SCHEMA_VERSION = 1

function digest(bytes) {
  return createHash("sha256").update(bytes).digest("hex")
}

// A file name inside an entry: a relative path of plain segments.
function entryName(name) {
  return name !== ENTRY_MANIFEST && name.split("/").every(part => part && part !== "." && part !== ".." && !part.includes("\\"))
}

// Takes a published cache entry: reads its manifest once, then reads each named file once into
// memory and checks what it read against that one manifest. Returns the bytes, or null (a miss)
// when anything is missing or differs. A publisher replacing the entry meanwhile can only turn a
// hit into a miss: the bytes returned are the bytes hashed, against one read of the manifest.
// With `names` null, the entry's files are the ones its manifest lists.
export async function takePublishedEntry(directory, names, { afterManifest, afterFile } = {}) {
  let manifest
  try {
    manifest = JSON.parse(await readFile(join(directory, ENTRY_MANIFEST), "utf8"))
  } catch {
    return null
  }
  const listed = manifest?.schemaVersion === ENTRY_SCHEMA_VERSION && manifest.files && typeof manifest.files === "object" ? Object.keys(manifest.files).sort() : null
  if (!listed || listed.length === 0 || !listed.every(entryName)) return null
  if (names !== null && JSON.stringify(listed) !== JSON.stringify([...names].sort())) return null
  await afterManifest?.()
  const taken = {}
  for (const name of names ?? listed) {
    const expected = manifest.files[name]
    if (typeof expected?.sha256 !== "string" || !Number.isSafeInteger(expected.size)) return null
    let bytes
    try {
      bytes = await readFile(join(directory, name))
    } catch {
      return null
    }
    if (bytes.length !== expected.size || digest(bytes) !== expected.sha256) return null
    taken[name] = bytes
    await afterFile?.(name)
  }
  return taken
}

// Publishes `files` (name to bytes, executable) as one entry with its manifest: written in a private
// folder of `temporaryRoot` (on the same file system as the entry), then renamed into place. An
// entry that `takePublishedEntry` accepts is kept and this build discarded when `accept` (the
// reader's own further checks) agrees; only an entry that fails is replaced, and it is moved into
// the private folder first.
export async function publishEntry(directory, files, { accept = async () => true, temporaryRoot }) {
  if (!temporaryRoot) throw new Error("publishEntry needs a temporary folder")
  const names = Object.keys(files)
  const usable = async () => {
    const taken = await takePublishedEntry(directory, names)
    return taken !== null && await accept(taken)
  }
  if (await usable()) return false
  await mkdir(dirname(directory), { recursive: true })
  const work = await makeWorkFolder(temporaryRoot)
  const staged = join(work, "entry")
  const replaced = join(work, "replaced")
  try {
    const manifest = { schemaVersion: ENTRY_SCHEMA_VERSION, files: {} }
    for (const name of names) {
      if (!entryName(name)) throw new Error(`Invalid cache entry file name: ${name}`)
      await mkdir(dirname(join(staged, name)), { recursive: true })
      await writeFile(join(staged, name), files[name], { mode: 0o755 })
      manifest.files[name] = { sha256: digest(files[name]), size: files[name].length }
    }
    await writeFile(join(staged, ENTRY_MANIFEST), `${JSON.stringify(manifest, null, 2)}\n`)
    await rename(directory, replaced).catch(error => { if (error.code !== "ENOENT") throw error })
    try {
      await rename(staged, directory)
    } catch (error) {
      // Another publisher put its entry in place between the two renames.
      if ((error.code === "ENOTEMPTY" || error.code === "EEXIST") && await usable()) return false
      throw error
    }
    return true
  } finally {
    await rm(work, { recursive: true, force: true })
  }
}
