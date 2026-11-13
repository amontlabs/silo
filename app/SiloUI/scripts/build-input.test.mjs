import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises"
import { spawn } from "node:child_process"
import { createServer } from "node:http"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { Readable } from "node:stream"
import test from "node:test"
import { fetchStream, fetchVerifiedFile, makeWorkFolder, otherLiveWork, ownerTag, publishEntry, removeAbandoned, runtimeCacheRoot, takePublishedEntry, verifyFile, writeVerifiedFile } from "./build-input.mjs"

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

test("a shared runtime cache root of another user is skipped for the checkout's own", { skip: process.getuid() === 0 && "runs as root" }, t => {
  const warn = t.mock.method(console, "warn", () => {})
  assert.equal(runtimeCacheRoot("/work/src-tauri", { SILO_RUNTIME_CACHE_DIR: "/" }), "/work/src-tauri/target/runtime-cache")
  assert.match(warn.mock.calls[0].arguments[0], /^\/ belongs to another user; SILO_RUNTIME_CACHE_DIR is skipped/)
})

// A child process that has created its own work folder and exits without removing it, as a killed
// run does. Returns the folder.
async function abandonedFolder(temporaryRoot) {
  const child = spawn(process.execPath, ["--input-type=module", "-e", `
import { makeWorkFolder } from ${JSON.stringify(new URL("./build-input.mjs", import.meta.url).href)}
console.log(await makeWorkFolder(${JSON.stringify(temporaryRoot)}))
process.kill(process.pid, "SIGKILL")
`], { stdio: ["ignore", "pipe", "inherit"] })
  let output = ""
  child.stdout.on("data", chunk => { output += chunk })
  await new Promise(resolve => child.on("close", resolve))
  return output.trim()
}

test("work folders of exited processes are removed by the next run; live ones are kept", async t => {
  const { root } = await setup(t)
  const temporaryRoot = join(root, "tmp")
  const abandoned = await abandonedFolder(temporaryRoot)
  assert.ok(abandoned.startsWith(temporaryRoot), abandoned)
  // A live process with a reused pid is a different owner: its start time differs.
  await mkdir(join(temporaryRoot, `${process.pid}-000000000000-reused`))
  await mkdir(join(temporaryRoot, "unnamed"))
  const own = await makeWorkFolder(temporaryRoot)
  assert.deepEqual(await readdir(temporaryRoot), [own.slice(temporaryRoot.length + 1)])
  // A staging name in a checkout folder is removed only when its owner has exited.
  await mkdir(join(root, `.staging-${ownerTag()}`))
  await mkdir(join(root, ".staging-1-000000000000"))
  await mkdir(join(root, "other-1-000000000000"))
  await removeAbandoned(root, ".staging-")
  assert.deepEqual((await readdir(root)).filter(name => name !== "archive.tar.gz" && name !== "tmp").sort(), [`.staging-${ownerTag()}`, "other-1-000000000000"])
})

test("a live owner is recognised whatever the time zone of the owner and the checker", async t => {
  const { root } = await setup(t)
  const temporaryRoot = join(root, "tmp")
  const child = spawn(process.execPath, ["--input-type=module", "-e", `
import { makeWorkFolder } from ${JSON.stringify(new URL("./build-input.mjs", import.meta.url).href)}
console.log(await makeWorkFolder(${JSON.stringify(temporaryRoot)}))
process.stdin.resume()
`], { stdio: ["pipe", "pipe", "inherit"], env: { ...process.env, TZ: "America/New_York" } })
  const exited = new Promise(resolve => child.on("close", resolve))
  t.after(() => { child.stdin.end(); return exited })
  const folder = await new Promise(resolve => child.stdout.once("data", chunk => resolve(chunk.toString().trim())))
  const zone = process.env.TZ
  process.env.TZ = "Asia/Tokyo"
  try {
    await removeAbandoned(temporaryRoot)
    assert.deepEqual(await readdir(temporaryRoot), [folder.slice(temporaryRoot.length + 1)])
    assert.deepEqual(await otherLiveWork(temporaryRoot), [folder])
  } finally {
    if (zone === undefined) delete process.env.TZ
    else process.env.TZ = zone
  }
  child.stdin.end()
  await exited
  await removeAbandoned(temporaryRoot)
  assert.deepEqual(await readdir(temporaryRoot), [])
})

test("abandoned work that cannot be removed is left in place instead of failing", { skip: process.getuid() === 0 && "runs as root" }, async t => {
  const { root } = await setup(t)
  const temporaryRoot = join(root, "tmp")
  const locked = join(temporaryRoot, "1-000000000000-x", "locked")
  await mkdir(locked, { recursive: true })
  await writeFile(join(locked, "file"), "")
  await chmod(locked, 0o555)
  const warn = t.mock.method(console, "warn", () => {})
  try {
    await removeAbandoned(temporaryRoot)
  } finally {
    await chmod(locked, 0o755)
  }
  assert.deepEqual(await readdir(temporaryRoot), ["1-000000000000-x"])
  assert.match(warn.mock.calls[0].arguments[0], /cannot be removed \(EACCES\); it is left in place/)
})

test("a published entry leaves nothing beside it or in the work folder", async t => {
  const { root } = await setup(t)
  const temporaryRoot = join(root, "tmp")
  const entry = join(root, "builds", "key")
  await publishEntry(entry, { tool: Buffer.from("first") }, { temporaryRoot })
  await publishEntry(entry, { tool: Buffer.from("second") }, { temporaryRoot, accept: async () => false })
  assert.deepEqual(await readdir(join(root, "builds")), ["key"])
  assert.deepEqual(await readdir(temporaryRoot), [])
  assert.equal((await takePublishedEntry(entry, ["tool"])).tool.toString(), "second")
  await fetchVerifiedFile(async () => Readable.from([bytes]), "https://fixture.invalid", digest, "fixture", join(root, "download"), { temporaryRoot })
  assert.deepEqual(await readdir(temporaryRoot), [])
})
