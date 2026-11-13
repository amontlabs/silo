import assert from "node:assert/strict"
import { execFile } from "node:child_process"
import { mkdir, mkdtemp, readdir, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import test from "node:test"
import { promisify } from "node:util"
import { publishEntry } from "./build-input.mjs"
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
  const run = async (command, environment = {}) => execute(process.execPath, [script, command, "--target", target], {
    env: { HOME: root, PATH: bin, SILO_RUNTIME_CACHE_DIR: cache, ...environment },
  })
  return { root, cache, run }
}

const parse = output => Object.fromEntries(output.trim().split("\n").map(line => line.split("=")))

test("the tool digest is stable and follows the build environment", async t => {
  const { run } = await fixture(t)
  const { digest } = parse((await run("tools")).stdout)
  assert.match(digest, /^[0-9a-f]{16}$/)
  assert.equal(parse((await run("tools")).stdout).digest, digest)
  assert.equal(parse((await run("tools", { CARGO_INCREMENTAL: "0", CARGO_PROFILE_DEV_DEBUG: "0" })).stdout).digest, digest)
  assert.notEqual(parse((await run("tools", { RUSTFLAGS: "-Copt-level=1" })).stdout).digest, digest)
})

test("keep-current keeps only this build's entries and fails when they are missing", async t => {
  const { cache, run } = await fixture(t)
  const keys = parse((await run("keys")).stdout)
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
  await assert.rejects(run("keep-current"), /No published MicroSandbox build/)
  await publishEntry(join(builds, keys.msb, "published"), msb, { temporaryRoot })
  await assert.rejects(run("keep-current"), /No published Git LFS transfer build/)
  await publishEntry(join(lfsBuilds, keys.lfs), lfs, { temporaryRoot })
  assert.deepEqual(parse((await run("keep-current")).stdout), keys)
  assert.deepEqual(await readdir(builds), [keys.msb])
  assert.deepEqual(await readdir(join(cache, "v0.0.1", "patched-builds")), [])
  assert.deepEqual(await readdir(lfsBuilds), [keys.lfs])
  assert.deepEqual(await readdir(join(cache, "git-lfs-transfer", "older-pin", "builds")), [])
})
