import test from "node:test"
import assert from "node:assert/strict"
import { copyFileSync, cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { spawnSync } from "node:child_process"
import { preflight } from "./preflight.mjs"
import { release } from "./release.mjs"

const source = resolve(dirname(fileURLToPath(import.meta.url)), "..")
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "silo-preflight-"))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  copyFileSync(join(source, "runtime-inputs.json"), join(root, "runtime-inputs.json"))
  mkdirSync(join(root, "patches"))
  const inputs = JSON.parse(readFileSync(join(root, "runtime-inputs.json"), "utf8"))
  for (const patch of inputs.patches) copyFileSync(join(source, patch.path), join(root, patch.path))
  return { root, inputs }
}

test("clean inputs validate without external commands or generated files", t => {
  const { root, inputs } = fixture(t)
  assert.deepEqual(preflight(root), inputs)
  assert.equal(inputs.patches.at(-1).path, "patches/microsandbox-metrics-sampler-blocking-0.7.6.patch")
})

const mutations = [
  ["patch bytes", (_, root, original) => writeFileSync(join(root, original.patches[0].path), "tampered patch")],
  ["patch pin", inputs => { inputs.patches[0].sha256 = "0".repeat(64) }],
  ["asset pin", inputs => { inputs.targets["aarch64-apple-darwin"].executableSha256 = "not-a-digest" }],
  ["missing target", inputs => { delete inputs.targets["x86_64-unknown-linux-gnu"] }],
  ["unsupported target", inputs => { inputs.targets["x86_64-apple-darwin"] = inputs.targets["aarch64-apple-darwin"] }],
  ["source revision", inputs => { inputs.sourceCommit = "0".repeat(40) }],
  ["feature set", inputs => { inputs.features = "net" }],
  ["schema", inputs => { inputs.schemaVersion = 1 }],
  ["unknown field", inputs => { inputs.patchSHA256 = inputs.patches[0].sha256 }],
  ["unsafe patch path", inputs => { inputs.patches[0].path = "../outside.patch" }],
  ["mismatched asset", inputs => { inputs.targets["aarch64-apple-darwin"].agentdAsset = "agentd-x86_64" }],
  ["inconsistent shared pin", inputs => { inputs.targets["aarch64-apple-darwin"].agentdSha256 = "0".repeat(64) }],
]
for (const [name, mutate] of mutations) {
  test(`${name} fails before tagging, native tools or downloads`, t => {
    const { root, inputs } = fixture(t)
    mutate(inputs, root, structuredClone(inputs))
    writeFileSync(join(root, "runtime-inputs.json"), JSON.stringify(inputs))
    assert.throws(() => preflight(root), /Runtime preflight/)
    mkdirSync(join(root, ".changeset"))
    // release() uses repo-relative notes; keep the whole fake repo under this temp root.
    const app = join(root, "repo/app/SiloUI")
    mkdirSync(app, { recursive: true })
    cpSync(join(root, "patches"), join(app, "patches"), { recursive: true })
    copyFileSync(join(root, "runtime-inputs.json"), join(app, "runtime-inputs.json"))
    mkdirSync(join(app, ".changeset"))
    mkdirSync(join(root, "repo/docs/releases"), { recursive: true })
    writeFileSync(join(root, "repo/docs/releases/0.98.7.md"), "Release notes")
    const run = (command, args) => {
      const key = [command, ...args].join(" ")
      const outputs = { "git status --porcelain": "", "python3 scripts/validate-release-version.py": "0.98.7", "git rev-parse HEAD": "head", "git tag --list v0.98.7": "" }
      assert.ok(Object.hasOwn(outputs, key), `Unexpected tag/build/download action: ${key}`)
      return outputs[key]
    }
    assert.throws(() => release("draft", app, run), /Runtime preflight/)
    cpSync(join(source, "scripts"), join(app, "scripts"), { recursive: true })
    const preparation = spawnSync(process.execPath, [join(app, "scripts/prepare-microsandbox-runtime.mjs")], { encoding: "utf8", env: { PATH: "", HOME: root } })
    assert.notEqual(preparation.status, 0)
    assert.match(preparation.stderr, /Runtime preflight/)
    assert.doesNotMatch(preparation.stderr, /rustc|Download failed/)
  })
}

test("symlinked patch cannot escape the checkout", t => {
  const { root, inputs } = fixture(t)
  const external = mkdtempSync(join(tmpdir(), "silo-outside-patch-"))
  t.after(() => rmSync(external, { recursive: true, force: true }))
  copyFileSync(join(root, inputs.patches[0].path), join(external, "patch"))
  rmSync(join(root, inputs.patches[0].path))
  symlinkSync(join(external, "patch"), join(root, inputs.patches[0].path))
  assert.throws(() => preflight(root), /containment/)
})

test("runtime preparation rejects a bad patch before invoking rustc or downloading", t => {
  const { root, inputs } = fixture(t)
  cpSync(join(source, "scripts"), join(root, "scripts"), { recursive: true })
  writeFileSync(join(root, inputs.patches[0].path), "bad patch")
  const result = spawnSync(process.execPath, [join(root, "scripts/prepare-microsandbox-runtime.mjs")], { encoding: "utf8", env: { PATH: "", HOME: root } })
  assert.notEqual(result.status, 0)
  assert.match(result.stderr, /Runtime preflight: invalid patches\[0\]\.sha256 mismatch/)
  assert.doesNotMatch(result.stderr, /rustc|Download failed/)
})


for (const [script, args, error] of [
  ["preflight.mjs", [], /Runtime preflight: invalid patches\[0\]\.sha256 mismatch/],
  ["release.mjs", ["invalid-action"], /Use npm run release:draft or npm run release:publish/],
  ["sync-release.mjs", [], /nonzero stable version/],
]) {
  test(`${script} executes its rejecting gate through a symlink`, t => {
    const { root, inputs } = fixture(t)
    cpSync(join(source, "scripts"), join(root, "scripts"), { recursive: true })
    writeFileSync(join(root, inputs.patches[0].path), "bad patch")
    writeFileSync(join(root, "package.json"), JSON.stringify({ version: "0.0.0" }))
    const link = join(root, `linked-${script}`)
    symlinkSync(join(root, "scripts", script), link)
    const result = spawnSync(process.execPath, [link, ...args], { encoding: "utf8", env: { PATH: "", HOME: root } })
    assert.equal(result.status, 1, result.stderr)
    assert.match(result.stderr, error)
  })
}
