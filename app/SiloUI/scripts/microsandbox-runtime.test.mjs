import test from "node:test"
import assert from "node:assert/strict"
import { spawn, execFileSync } from "node:child_process"
import { chmod, copyFile, mkdir, symlink, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { setTimeout as delay } from "node:timers/promises"
import { Readable } from "node:stream"
import { MICRO_SANDBOX_VERSION, MICROSANDBOX_PATCHES, patchedBuildKey, readBuildToolIdentity, readMacosGatewayInputs, runtimeTargets, sha256, stageRuntime } from "./microsandbox-runtime.mjs"

const runtimeModule = new URL("./microsandbox-runtime.mjs", import.meta.url).href
const toolScript = `#!${process.execPath}
import { mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { setTimeout as delay } from "node:timers/promises"
const args = process.argv.slice(2)
if (process.argv[1].endsWith("rustc")) {
  console.log("rustc 1.94.0 (fixture)")
} else if (args.includes("fetch")) {
  if (process.env.FIXTURE_ROLE === "failure") throw new Error("Fixture compiler failure")
  if (process.env.FIXTURE_ROLE === "first") {
    writeFileSync(join(process.env.FIXTURE_ROOT, "ready"), process.cwd())
    const deadline = Date.now() + 30000
    while (true) {
      try { readFileSync(join(process.env.FIXTURE_ROOT, "continue")); break } catch {}
      if (Date.now() > deadline) throw new Error("Fixture gate timed out")
      await delay(10)
    }
  }
  if (readFileSync("source.txt", "utf8") !== "fixture source") throw new Error("Source was replaced")
} else if (args.includes("--bin")) {
  const target = args[args.indexOf("--target") + 1]
  const output = join(process.env.CARGO_TARGET_DIR, target, "release")
  mkdirSync(output, { recursive: true })
  writeFileSync(join(output, "silo-macos-gateway"), "gateway " + (process.env.FIXTURE_VARIANT ?? ""), { mode: 0o755 })
} else {
  const target = args[args.indexOf("--target") + 1]
  const output = join(process.env.CARGO_TARGET_DIR, target, "release")
  mkdirSync(output, { recursive: true })
  writeFileSync(join(output, "msb"), ${JSON.stringify(`#!${process.execPath}
const args = process.argv.slice(2)
if (args[0] === "--version") console.log("msb ${MICRO_SANDBOX_VERSION}")
else if (args[0].startsWith("--silo-")) console.log("1")
else console.log("--authorized-keys --exit-on-stdin-close --expected-machine-id --stage-id --no-start --no-stdin --mount-owned --progress-json --from-sandbox --group --dest-dir --full --guest-flush --integrity --cow-mem --name --net-strict")
`)} + "// variant " + (process.env.FIXTURE_VARIANT ?? "") + "\\n", { mode: 0o755 })
}
`

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), "silo-runtime-concurrency-"))
  t.after(() => rm(root, { recursive: true, force: true }))
  const bin = join(root, "bin")
  const source = join(root, "source")
  await mkdir(bin)
  await mkdir(source)
  await writeFile(join(source, "source.txt"), "fixture source")
  const archive = join(root, "source.tar.gz")
  execFileSync("/usr/bin/tar", ["-czf", archive, "-C", root, "source"])
  for (const tool of ["rustc", "cargo"]) await writeFile(join(bin, tool), toolScript, { mode: 0o755 })
  // GNU tar decompresses through an external gzip found on PATH, and the workers run with only this directory on PATH.
  await symlink(execFileSync("/bin/sh", ["-c", "command -v gzip"], { encoding: "utf8" }).trim(), join(bin, "gzip"))
  const app = join(root, "app")
  await mkdir(join(app, "src-tauri/macos-gateway/src"), { recursive: true })
  await writeFile(join(app, "src-tauri/macos-gateway/Cargo.toml"), "[package]")
  await writeFile(join(app, "src-tauri/macos-gateway/src/main.rs"), "fn main() {}")
  const worker = join(root, "worker.mjs")
  await writeFile(worker, `
import { existsSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { setTimeout as delay } from "node:timers/promises"
import { buildPatchedExecutable } from ${JSON.stringify(runtimeModule)}
const pause = async point => {
  if (process.env.FIXTURE_PAUSE !== point) return
  writeFileSync(join(process.env.FIXTURE_ROOT, "paused"), point)
  while (!existsSync(join(process.env.FIXTURE_ROOT, "resume"))) await delay(10)
}
const output = await buildPatchedExecutable({
  appRoot: process.env.FIXTURE_APP || undefined,
  targetTriple: "aarch64-apple-darwin", hostTriple: "aarch64-apple-darwin",
  sourceArchive: ${JSON.stringify(archive)}, patches: [],
  agentd: Buffer.from("fixture agent"), cacheRoot: ${JSON.stringify(join(root, "cache"))},
  hooks: { afterManifest: () => pause("afterManifest"), afterFile: name => pause("after-" + name) },
})
const bytes = output.executable ?? output
if (!bytes.length) throw new Error("No executable returned")
writeFileSync(join(process.env.FIXTURE_ROOT, "result-" + (process.env.FIXTURE_VARIANT ?? "")), JSON.stringify({
  msb: bytes.toString("utf8"), gateway: output.macosGateway?.toString("utf8") ?? null,
}))
`)
  return { root, bin, worker, app }
}

function runWorker(t, fixture, role, environment = {}) {
  const child = spawn(process.execPath, [fixture.worker], {
    env: { ...process.env, PATH: fixture.bin, HOME: fixture.root, FIXTURE_ROOT: fixture.root, FIXTURE_ROLE: role, ...environment },
    stdio: ["ignore", "pipe", "pipe"],
  })
  t.after(() => { if (child.exitCode === null && child.signalCode === null) child.kill("SIGTERM") })
  let output = ""
  child.stdout.on("data", chunk => { output += chunk })
  child.stderr.on("data", chunk => { output += chunk })
  return new Promise((resolve, reject) => {
    child.on("error", reject)
    child.on("close", code => resolve({ code, output }))
  })
}

async function waitForFile(path) {
  const deadline = Date.now() + 15000
  while (Date.now() < deadline) {
    // The file exists before its contents are written.
    try {
      const content = await readFile(path, "utf8")
      if (content) return content
    } catch {}
    await delay(10)
  }
  throw new Error(`Timed out waiting for ${path}`)
}

test("a completed preparation cannot remove another process's active source", { timeout: 45000 }, async t => {
  const paths = await fixture(t)
  const first = runWorker(t, paths, "first")
  const firstSource = await waitForFile(join(paths.root, "ready"))
  try {
    const second = await runWorker(t, paths, "second")
    assert.equal(second.code, 0, second.output)
    assert.equal(await readFile(join(firstSource, "source.txt"), "utf8"), "fixture source")
  } finally {
    await writeFile(join(paths.root, "continue"), "continue")
  }
  const result = await first
  assert.equal(result.code, 0, result.output)
  await assert.rejects(readFile(join(firstSource, "source.txt")), { code: "ENOENT" })
})


test("a failed preparation removes its own extracted source", async t => {
  const paths = await fixture(t)
  const result = await runWorker(t, paths, "failure")
  assert.notEqual(result.code, 0)
  assert.match(result.output, /Fixture compiler failure/)
  // Neither the work folders nor an empty folder for the failed key remain.
  assert.deepEqual(await readdir(join(paths.root, "cache", "patched-builds")), [])
})


test("failed capability verification removes staging files and preserves the published runtime", async t => {
  const { root } = await fixture(t)
  const appRoot = join(root, "app")
  const targetTriple = "aarch64-apple-darwin"
  const executable = Buffer.from(`#!${process.execPath}\nconsole.log("unsupported runtime")\n`)
  const agentd = Buffer.from("fixture agent")
  const library = Buffer.from("fixture library")
  const source = Buffer.from("fixture source")
  const selected = {
    ...runtimeTargets[targetTriple], executableSha256: sha256(executable),
    agentdSha256: sha256(agentd), librarySha256: sha256(library),
  }
  const sourceArtifact = { url: "https://fixture.invalid/source", sha256: sha256(source) }
  await mkdir(join(appRoot, "patches"), { recursive: true })
  for (const patch of MICROSANDBOX_PATCHES) {
    await copyFile(new URL(`../${patch.path}`, import.meta.url), join(appRoot, patch.path))
  }
  const binaries = join(appRoot, "src-tauri/binaries")
  const resources = join(appRoot, "src-tauri/runtime")
  const published = join(resources, "microsandbox")
  await mkdir(binaries, { recursive: true })
  await mkdir(published, { recursive: true })
  await writeFile(join(binaries, `msb-${targetTriple}`), "previous executable")
  await writeFile(join(published, "manifest.json"), "previous manifest")
  const fetchStream = async url => {
    const bytes = url === sourceArtifact.url ? source : url.endsWith(selected.agentdAsset) ? agentd
      : url.endsWith(selected.libraryAsset) ? library : executable
    return Readable.from([bytes])
  }
  await assert.rejects(stageRuntime({
    appRoot, targetTriple, selected, sourceArtifact, licenses: [], fetchStream,
    buildExecutable: async () => executable,
  }), /failed its version/)
  assert.equal(await readFile(join(binaries, `msb-${targetTriple}`), "utf8"), "previous executable")
  assert.equal(await readFile(join(published, "manifest.json"), "utf8"), "previous manifest")
  assert.deepEqual(await readdir(binaries), [`msb-${targetTriple}`])
  assert.deepEqual(await readdir(resources), ["microsandbox"])
})


test("a cached executable without its execute bit is still served: the bytes are what is used", async t => {
  const paths = await fixture(t)
  const first = await runWorker(t, paths, "second", { FIXTURE_VARIANT: "A" })
  assert.equal(first.code, 0, first.output)
  const builds = join(paths.root, "cache", "patched-builds")
  const [key] = await readdir(builds)
  await chmod(join(builds, key, "published", "msb"), 0o644)
  const result = await runWorker(t, paths, "second", { FIXTURE_VARIANT: "B" })
  assert.equal(result.code, 0, result.output)
  assert.match(JSON.parse(await readFile(join(paths.root, "result-B"), "utf8")).msb, /variant A/)
})


test("a manifest-valid cached executable that fails its probes is rebuilt and replaced", async t => {
  const paths = await fixture(t)
  const first = await runWorker(t, paths, "second", { FIXTURE_VARIANT: "A" })
  assert.equal(first.code, 0, first.output)
  const builds = join(paths.root, "cache", "patched-builds")
  const [key] = await readdir(builds)
  const published = join(builds, key, "published")
  const broken = Buffer.from(`#!${process.execPath}\nprocess.exit(17)\n`)
  await writeFile(join(published, "msb"), broken)
  await writeFile(join(published, "manifest.json"), JSON.stringify({ schemaVersion: 1, files: { msb: { sha256: sha256(broken), size: broken.length } } }))
  const result = await runWorker(t, paths, "second", { FIXTURE_VARIANT: "B" })
  assert.equal(result.code, 0, result.output)
  assert.match(JSON.parse(await readFile(join(paths.root, "result-B"), "utf8")).msb, /variant B/)
  const manifest = JSON.parse(await readFile(join(published, "manifest.json"), "utf8"))
  const msb = await readFile(join(published, "msb"))
  assert.equal(manifest.files.msb.sha256, sha256(msb))
  assert.match(msb.toString("utf8"), /variant B/)
})


// A second process publishes pair B under the same key while a reader holds pair A's manifest.
// The publisher is told A fails (as when it found a damaged pair, or checked before A appeared).
async function publishInAnotherProcess(t, paths, published, variant) {
  const script = join(paths.root, `publish-${variant}.mjs`)
  // A working program, so a reader that wrongly took it would return it.
  const program = (await readFile(join(published, "msb"), "utf8")).replace(/variant \w+/, `variant ${variant}`)
  await writeFile(script, `
import { publishEntry } from ${JSON.stringify(new URL("./build-input.mjs", import.meta.url).href)}
const msb = Buffer.from(${JSON.stringify(program)})
await publishEntry(${JSON.stringify(published)}, { msb, "silo-macos-gateway": Buffer.from("gateway ${variant}") }, { accept: async () => false })
`)
  const child = spawn(process.execPath, [script], { stdio: ["ignore", "pipe", "pipe"] })
  let output = ""
  child.stderr.on("data", chunk => { output += chunk })
  const code = await new Promise(resolve => child.on("close", resolve))
  assert.equal(code, 0, output)
}

for (const point of ["afterManifest", "after-msb"]) {
  test(`a reader paused at ${point} while another process publishes ends in a miss, never a mixed pair`, { timeout: 45000 }, async t => {
    const paths = await fixture(t)
    const first = await runWorker(t, paths, "second", { FIXTURE_APP: paths.app, FIXTURE_VARIANT: "A" })
    assert.equal(first.code, 0, first.output)
    const builds = join(paths.root, "cache", "patched-builds")
    const [key] = await readdir(builds)
    const published = join(builds, key, "published")
    const reader = runWorker(t, paths, "second", { FIXTURE_APP: paths.app, FIXTURE_VARIANT: "R", FIXTURE_PAUSE: point })
    try {
      assert.equal(await waitForFile(join(paths.root, "paused")), point)
      await publishInAnotherProcess(t, paths, published, "B")
      assert.equal((await readFile(join(published, "silo-macos-gateway"), "utf8")), "gateway B")
    } finally {
      await writeFile(join(paths.root, "resume"), "")
    }
    const result = await reader
    assert.equal(result.code, 0, result.output)
    // The reader missed and built its own pair; it did not return A's, B's or a mix.
    assert.deepEqual(JSON.parse(await readFile(join(paths.root, "result-R"), "utf8")).gateway, "gateway R")
    assert.match(JSON.parse(await readFile(join(paths.root, "result-R"), "utf8")).msb, /variant R/)
    // B is a valid pair, so the reader's own publish kept it and discarded its build.
    assert.equal((await readFile(join(published, "silo-macos-gateway"), "utf8")), "gateway B")
    assert.match(await readFile(join(published, "msb"), "utf8"), /variant B/)
  })
}


test("a reader with no publish in between takes the published pair", async t => {
  const paths = await fixture(t)
  const first = await runWorker(t, paths, "second", { FIXTURE_APP: paths.app, FIXTURE_VARIANT: "A" })
  assert.equal(first.code, 0, first.output)
  const second = await runWorker(t, paths, "second", { FIXTURE_APP: paths.app, FIXTURE_VARIANT: "R" })
  assert.equal(second.code, 0, second.output)
  assert.deepEqual(JSON.parse(await readFile(join(paths.root, "result-R"), "utf8")).gateway, "gateway A")
})


test("each build tool identity part changes the cache key", async t => {
  const root = await mkdtemp(join(tmpdir(), "silo-build-key-"))
  t.after(() => rm(root, { recursive: true, force: true }))
  const builds = join(root, "work/cache/patched-builds")
  await mkdir(builds, { recursive: true })
  const cargoHome = join(root, "cargo-home")
  await mkdir(cargoHome)
  const versions = { "xcode-select": "/Applications/Xcode.app/Contents/Developer", xcodebuild: "Xcode 26.5", xcrun: "26.5", cc: "Apple clang 21", ld: "ld-1267" }
  const output = executable => versions[executable]
  const key = async (environment = {}) => patchedBuildKey({
    agentd: Buffer.from("agent"), rustcVersion: "rustc 1.100.0", targetTriple: "aarch64-apple-darwin", gatewaySourceDigest: "", recipeSha256: "",
    tools: await readBuildToolIdentity({ cargoDirectory: builds, environment: { HOME: root, CARGO_HOME: cargoHome, ...environment }, output }),
  })
  const base = await key()
  assert.equal(await key(), base)
  assert.equal(await key({ CARGO_TERM_COLOR: "always", CARGO_BUILD_JOBS: "3", CARGO_TARGET_DIR: "/elsewhere", PATH: "/x" }), base)
  const changed = []
  for (const tool of Object.keys(versions)) {
    const previous = versions[tool]
    versions[tool] = `${previous} (other)`
    changed.push([tool, await key()])
    versions[tool] = previous
  }
  for (const [name, value] of [["RUSTFLAGS", "-C target-cpu=native"], ["CARGO_ENCODED_RUSTFLAGS", "-Copt-level=1"], ["CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER", "/x/ld"], ["CARGO_PROFILE_RELEASE_LTO", "false"], ["CC_aarch64_apple_darwin", "/x/cc"], ["SDKROOT", "/x/sdk"]]) {
    changed.push([name, await key({ [name]: value })])
  }
  await writeFile(join(cargoHome, "config.toml"), "[build]\nrustflags = [\"-Cdebuginfo=1\"]\n")
  changed.push(["cargo home config.toml", await key()])
  await mkdir(join(root, "work/.cargo"))
  await writeFile(join(root, "work/.cargo/config"), "[profile.release]\nopt-level = 2\n")
  changed.push(["parent .cargo/config", await key()])
  const keys = [base, ...changed.map(([, value]) => value)]
  assert.equal(new Set(keys).size, keys.length, JSON.stringify(changed))
})


test("every file of the gateway crate except its build output feeds the cache key", async t => {
  const appRoot = await mkdtemp(join(tmpdir(), "silo-gateway-inputs-"))
  t.after(() => rm(appRoot, { recursive: true, force: true }))
  const gateway = join(appRoot, "src-tauri/macos-gateway")
  for (const name of ["Cargo.toml", "build.rs", "src/main.rs", "src/later/module.rs", "target/debug/ignored"]) {
    await mkdir(join(gateway, name, ".."), { recursive: true })
    await writeFile(join(gateway, name), name)
  }
  const names = (await readMacosGatewayInputs(appRoot)).map(([name]) => name)
  assert.deepEqual(names, ["Cargo.toml", "build.rs", "src/later/module.rs", "src/main.rs"])
})
