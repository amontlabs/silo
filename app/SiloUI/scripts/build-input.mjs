import { createHash } from "node:crypto"
import { createReadStream, createWriteStream } from "node:fs"
import { mkdir, mkdtemp, readFile, rename, rm, stat, writeFile } from "node:fs/promises"
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

export async function writeVerifiedFile(path, openSource, expectedSha256, label, limits = {}) {
  const verification = verifier(expectedSha256, label, limits)
  const signal = AbortSignal.timeout(limits.timeoutMs ?? INPUT_TIMEOUT_MS)
  await mkdir(dirname(path), { recursive: true })
  const temporaryRoot = await mkdtemp(join(dirname(path), `.${basename(path)}.tmp-`))
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

// Where preparation keeps downloads and content-keyed builds. SILO_RUNTIME_CACHE_DIR (absolute)
// lets every checkout of a machine share one root; otherwise each checkout keeps its own.
export function runtimeCacheRoot(tauriRoot, environment = process.env) {
  const shared = environment.SILO_RUNTIME_CACHE_DIR?.trim()
  if (!shared) return join(tauriRoot, "target", "runtime-cache")
  if (!isAbsolute(shared)) throw new Error("SILO_RUNTIME_CACHE_DIR must be an absolute path")
  return resolve(shared)
}

const ENTRY_MANIFEST = "manifest.json"

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
  const listed = manifest?.schemaVersion === 1 && manifest.files && typeof manifest.files === "object" ? Object.keys(manifest.files).sort() : null
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
// folder beside the entry, then renamed into place. An entry that `takePublishedEntry` accepts is
// kept and this build discarded when `accept` (the reader's own further checks) agrees; only an
// entry that fails is replaced.
export async function publishEntry(directory, files, { accept = async () => true } = {}) {
  const names = Object.keys(files)
  const usable = async () => {
    const taken = await takePublishedEntry(directory, names)
    return taken !== null && await accept(taken)
  }
  if (await usable()) return false
  await mkdir(dirname(directory), { recursive: true })
  const staged = await mkdtemp(`${directory}.publish-`)
  const replaced = `${staged}.replaced`
  try {
    const manifest = { schemaVersion: 1, files: {} }
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
    await rm(staged, { recursive: true, force: true })
    await rm(replaced, { recursive: true, force: true })
  }
}
