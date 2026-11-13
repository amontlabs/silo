import assert from "node:assert/strict"
import { execFile } from "node:child_process"
import { mkdir, mkdtemp, readdir, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import test from "node:test"
import { promisify } from "node:util"
import { RUNTIME_CACHE_FORMAT, makeWorkFolder, publishEntry } from "./build-input.mjs"
import { DUGITE_RELEASE } from "./git-runtime.mjs"
import { LFS_TRANSFER_COMMIT } from "./lfs-transfer-runtime.mjs"
import { MICRO_SANDBOX_VERSION } from "./microsandbox-runtime.mjs"

const execute = promisify(execFile)
const script = new URL("./runtime-cache-entries.mjs", import.meta.url).pathname
const target = "aarch64-apple-darwin"

// rustc and go stand-ins on an otherwise empty PATH, and a temporary cache root.
async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), "silo-runtime-entries-"))
  t.after(() => rm(root, { recursive: true, force: true }))
  const bin = join(root, "bin")
  await mkdir(bin)
  await writeFile(join(bin, "rustc"), `#!${process.execPath}\nconsole.log("rustc 1.100.0 (fixture)")\n`, { mode: 0o755 })
  await writeFile(join(bin, "go"), `#!${process.execPath}\nconsole.log(JSON.stringify({ GOVERSION: "go1.25.1", GOROOT: "/fixture/go", GOTOOLCHAIN: "local", GOFLAGS: "", GOEXPERIMENT: "", GOAMD64: "v1", GOARM64: "v8.0", GOFIPS140: "off" }))\n`, { mode: 0o755 })
  const cache = join(root, "cache")
  const run = async (command, environment = {}, options = []) => execute(process.execPath, [script, command, "--target", target, ...options], {
    env: { HOME: root, PATH: bin, SILO_RUNTIME_CACHE_DIR: cache, ...environment },
  })
  return { root, cache, run }
}

const parse = output => Object.fromEntries(output.trim().split("\n").map(line => line.split("=")))

test("the tool digest is stable and follows the build environment", async t => {
  const { run } = await fixture(t)
  const { digest, format } = parse((await run("tools")).stdout)
  assert.match(digest, /^[0-9a-f]{16}$/)
  assert.equal(format, String(RUNTIME_CACHE_FORMAT))
  assert.equal(parse((await run("tools")).stdout).digest, digest)
  assert.equal(parse((await run("tools", { CARGO_INCREMENTAL: "0", CARGO_PROFILE_DEV_DEBUG: "0" })).stdout).digest, digest)
  assert.notEqual(parse((await run("tools", { RUSTFLAGS: "-Copt-level=1" })).stdout).digest, digest)
})

// This build's msb and Git LFS entries in `cache`, and the folders an older build left there.
async function populate(cache, keys) {
  const temporaryRoot = join(cache, "tmp")
  const builds = join(cache, `v${MICRO_SANDBOX_VERSION}`, "patched-builds")
  const lfsBuilds = join(cache, "git-lfs-transfer", LFS_TRANSFER_COMMIT, "builds")
  const msb = { msb: Buffer.from("msb"), "silo-macos-gateway": Buffer.from("gateway") }
  const lfs = { "git-lfs-transfer": Buffer.from("lfs"), "LICENSE-MIT.txt": Buffer.from("MIT"), "licenses/Go-LICENSE": Buffer.from("Go") }
  await publishEntry(join(builds, "older", "published"), msb, { temporaryRoot })
  await publishEntry(join(cache, "v0.0.1", "patched-builds", "older", "published"), msb, { temporaryRoot })
  await publishEntry(join(lfsBuilds, "linux-arm64-older"), lfs, { temporaryRoot })
  await publishEntry(join(cache, "git-lfs-transfer", "older-pin", "builds", "linux-arm64-older"), lfs, { temporaryRoot })
  await mkdir(join(lfsBuilds, "linux-arm64"))
  await mkdir(join(cache, "dugite", DUGITE_RELEASE), { recursive: true })
  await mkdir(join(cache, "dugite", "v0.0.1"))
  return {
    builds, lfsBuilds,
    publishMsb: () => publishEntry(join(builds, keys.msb, "published"), msb, { temporaryRoot }),
    publishLfs: () => publishEntry(join(lfsBuilds, keys.lfs), lfs, { temporaryRoot }),
  }
}

test("keep-current keeps only this build's entries and inputs and fails when they are missing", async t => {
  const { cache, run } = await fixture(t)
  const keys = parse((await run("keys")).stdout)
  const { builds, lfsBuilds, publishMsb, publishLfs } = await populate(cache, keys)
  const ci = { CI: "true" }
  await assert.rejects(run("keep-current", ci), /No published MicroSandbox build/)
  await publishMsb()
  await assert.rejects(run("keep-current", ci), /No published Git LFS transfer build/)
  await publishLfs()
  assert.deepEqual(parse((await run("keep-current", ci)).stdout), keys)
  assert.deepEqual(await readdir(builds), [keys.msb])
  assert.deepEqual(await readdir(lfsBuilds), [keys.lfs])
  assert.deepEqual((await readdir(cache)).sort(), ["dugite", "git-lfs-transfer", "tmp", `v${MICRO_SANDBOX_VERSION}`])
  assert.deepEqual(await readdir(join(cache, "git-lfs-transfer")), [LFS_TRANSFER_COMMIT])
  assert.deepEqual(await readdir(join(cache, "dugite")), [DUGITE_RELEASE])
})

test("keep-current runs only in CI or when told the shared cache is this checkout's", async t => {
  const { cache, run } = await fixture(t)
  const keys = parse((await run("keys")).stdout)
  const { publishMsb, publishLfs } = await populate(cache, keys)
  await publishMsb()
  await publishLfs()
  await assert.rejects(run("keep-current"), error => error.code === 2 && /pass --yes-shared-cache-is-mine/.test(error.stderr))
  assert.ok((await readdir(cache)).includes("v0.0.1"))
  await assert.rejects(run("tools", {}, ["--yes-shared-cache-is-mine"]), error => error.code === 2)
  assert.deepEqual(parse((await run("keep-current", {}, ["--yes-shared-cache-is-mine"])).stdout), keys)
  assert.ok(!(await readdir(cache)).includes("v0.0.1"))
})

test("keep-current removes nothing while another process prepares from the same cache", async t => {
  const { cache, run } = await fixture(t)
  const keys = parse((await run("keys")).stdout)
  const { builds, publishMsb, publishLfs } = await populate(cache, keys)
  await publishMsb()
  await publishLfs()
  // This test process stands for another preparation that is still running.
  const work = await makeWorkFolder(join(cache, "tmp"))
  const { stderr } = await run("keep-current", { CI: "true" })
  assert.match(stderr, /Another process is preparing from this runtime cache, so nothing was removed/)
  assert.deepEqual((await readdir(builds)).sort(), [keys.msb, "older"].sort())
  assert.ok((await readdir(cache)).includes("v0.0.1"))
  await rm(work, { recursive: true })
  await run("keep-current", { CI: "true" })
  assert.deepEqual(await readdir(builds), [keys.msb])
})
