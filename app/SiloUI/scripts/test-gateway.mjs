// Runs the gateway crate's own tests (src-tauri/macos-gateway) on this host. The crate builds
// only inside the pinned, patched MicroSandbox checkout, so this stages that checkout like the
// runtime build does (verified source archive, Silo's patches, the gateway sources beside the
// MicroSandbox crates), then runs `cargo test` there.
//
//   node scripts/test-gateway.mjs [-- cargo test arguments]
//
// The source archive comes from the runtime cache that `runtime:prepare` fills; a cold cache
// downloads it, checked against the pin in runtime-inputs.json.
import { execFileSync } from "node:child_process"
import { cp, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"

import { fetchStream, fetchVerifiedFile } from "./build-input.mjs"
import {
  MICRO_SANDBOX_VERSION, MICROSANDBOX_BUILD_TOOLCHAIN, MICROSANDBOX_PATCHES,
  MICROSANDBOX_SOURCE_SHA256, MICROSANDBOX_SOURCE_URL, applyRuntimePatch, verifySha256,
} from "./microsandbox-runtime.mjs"

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..")
const sourceCommit = JSON.parse(await readFile(join(appRoot, "runtime-inputs.json"), "utf8")).sourceCommit
const cacheRoot = join(appRoot, "src-tauri/target/runtime-cache", `v${MICRO_SANDBOX_VERSION}`)
const archive = await fetchVerifiedFile(
  fetchStream, MICROSANDBOX_SOURCE_URL, MICROSANDBOX_SOURCE_SHA256, "MicroSandbox pinned source",
  join(cacheRoot, `microsandbox-${sourceCommit}.tar.gz`),
)

const work = await mkdtemp(join(tmpdir(), "silo-gateway-test-"))
try {
  execFileSync("tar", ["-xzf", archive, "-C", work], { stdio: "inherit" })
  const [directory] = (await readdir(work, { withFileTypes: true })).filter(entry => entry.isDirectory())
  const source = join(work, directory.name)
  for (const [index, patch] of MICROSANDBOX_PATCHES.entries()) {
    const bytes = await readFile(resolve(appRoot, patch.path))
    verifySha256(bytes, patch.sha256, `Silo runtime patch ${index + 1}`)
    const patchPath = join(work, `patch-${index}.patch`)
    await writeFile(patchPath, bytes)
    applyRuntimePatch(source, patchPath)
  }
  await cp(resolve(appRoot, "src-tauri/macos-gateway"), join(source, "macos-gateway"), { recursive: true })
  const extra = process.argv.slice(2).filter(argument => argument !== "--")
  execFileSync("cargo", [
    `+${MICROSANDBOX_BUILD_TOOLCHAIN}`, "test", "--locked",
    "--manifest-path", join(source, "macos-gateway/Cargo.toml"), ...extra,
  ], { stdio: "inherit", cwd: source, env: { ...process.env, CARGO_TARGET_DIR: join(work, "target") } })
} finally {
  await rm(work, { recursive: true, force: true })
}
