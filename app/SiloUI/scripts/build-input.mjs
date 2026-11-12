import { createHash } from "node:crypto"
import { createReadStream, createWriteStream } from "node:fs"
import { mkdir, mkdtemp, rename, rm, stat } from "node:fs/promises"
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
