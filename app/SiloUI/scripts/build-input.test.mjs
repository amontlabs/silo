import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises"
import { createServer } from "node:http"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { Readable } from "node:stream"
import test from "node:test"
import { fetchStream, fetchVerifiedFile, runtimeCacheRoot, verifyFile, writeVerifiedFile } from "./build-input.mjs"

const bytes = Buffer.from("verified build input")
const digest = createHash("sha256").update(bytes).digest("hex")
async function setup(t) {
  const root = await mkdtemp(join(tmpdir(), "silo-build-input-"))
  t.after(() => rm(root, { recursive: true, force: true }))
  const file = join(root, "archive.tar.gz")
  await writeFile(file, "previous")
  return { root, file }
}

test("an oversized stream stops early and keeps the published input", async t => {
  const { root, file } = await setup(t)
  let chunks = 0
  const source = Readable.from((function* () {
    for (let i = 0; i < 1000; i += 1) { chunks += 1; yield bytes }
  })(), { objectMode: false, highWaterMark: bytes.length })
  await assert.rejects(writeVerifiedFile(file, async () => source, digest, "fixture", { maxBytes: bytes.length }), /size limit/)
  assert.ok(chunks < 10, `Consumed ${chunks} chunks after the limit`)
  assert.equal(source.destroyed, true)
  assert.equal(await readFile(file, "utf8"), "previous")
  assert.deepEqual(await readdir(root), ["archive.tar.gz"])
})

test("stream verification rejects a bad exact length even when the hash matches", async t => {
  const { file } = await setup(t)
  await writeFile(file, bytes)
  await assert.rejects(verifyFile(file, digest, "fixture", { bytes: bytes.length + 1 }), /checksum mismatch/)
})

test("file write failure closes the source and removes staging files", async t => {
  const { file, root } = await setup(t)
  const source = Readable.from([bytes])
  // Renaming over a directory fails after the input has been verified.
  const destination = root
  await assert.rejects(writeVerifiedFile(destination, async () => source, digest, "fixture"))
  assert.equal(source.destroyed, true)
  assert.equal(await readFile(file, "utf8"), "previous")
})

for (const mode of ["headers", "body", "status", "chunked"]) {
  test(`HTTP ${mode} obeys the deadline and publication policy`, { timeout: 10_000 }, async t => {
    const { root, file } = await setup(t)
    const deadlines = []
    const timeout = t.mock.method(AbortSignal, "timeout", () => {
      const deadline = new AbortController()
      deadlines.push(deadline)
      return deadline.signal
    })
    const expire = () => deadlines.at(-1).abort(new DOMException("Fixture deadline expired", "TimeoutError"))
    const requested = Promise.withResolvers()
    const server = createServer((req, res) => {
      requested.resolve()
      if (mode === "headers") return
      if (mode === "status") { res.writeHead(503); res.end("unavailable"); return }
      res.writeHead(200)
      if (mode === "body") { res.write(bytes.subarray(0, 3)); return }
      res.write(bytes.subarray(0, 3))
      res.end(bytes.subarray(3))
    })
    t.after(() => { server.closeAllConnections(); server.close() })
    await new Promise(resolve => server.listen(0, "127.0.0.1", resolve))
    const url = `http://127.0.0.1:${server.address().port}/archive`
    const fetchInput = async (url, options) => {
      const source = await fetchStream(url, options)
      if (mode === "body") source.once("data", expire)
      return source
    }
    const operation = fetchVerifiedFile(fetchInput, url, digest, "fixture", file, { bytes: bytes.length, timeoutMs: 100 })
    if (mode === "chunked") {
      assert.equal(await operation, file)
      assert.deepEqual(await readFile(file), bytes)
      await fetchVerifiedFile(async () => { throw new Error("Must reuse cache") }, url, digest, "fixture", file, { bytes: bytes.length })
    } else {
      const rejected = assert.rejects(operation, mode === "status" ? /Download failed \(503\)/ : /abort|timeout/i)
      if (mode === "headers") {
        await requested.promise
        expire()
      }
      await rejected
      assert.equal(await readFile(file, "utf8"), "previous")
    }
    assert.deepEqual(timeout.mock.calls.slice(0, 2).map(call => call.arguments[0]), [100, 100])
    assert.deepEqual(await readdir(root), ["archive.tar.gz"])
  })
}

test("the runtime cache root is per checkout unless an absolute shared root is configured", () => {
  assert.equal(runtimeCacheRoot("/work/src-tauri", {}), "/work/src-tauri/target/runtime-cache")
  assert.equal(runtimeCacheRoot("/work/src-tauri", { SILO_RUNTIME_CACHE_DIR: " /shared/silo " }), "/shared/silo")
  assert.throws(() => runtimeCacheRoot("/work/src-tauri", { SILO_RUNTIME_CACHE_DIR: "relative" }), /absolute/)
})
