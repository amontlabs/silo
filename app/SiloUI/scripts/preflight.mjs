import { createHash } from "node:crypto"
import { existsSync, readFileSync, realpathSync } from "node:fs"
import { dirname, relative, resolve, sep } from "node:path"
import { fileURLToPath } from "node:url"

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..")
const digest = /^[a-f0-9]{64}$/
const revision = /^[a-f0-9]{40}$/
const version = /^\d+\.\d+\.\d+$/
function requireValue(valid, field) {
  if (!valid) throw new Error(`Runtime preflight: invalid ${field}`)
}
function keys(value, expected, field) {
  requireValue(value && typeof value === "object" && !Array.isArray(value)
    && Object.keys(value).sort().join() === [...expected].sort().join(), field)
}
function matches(value, pattern, field) {
  requireValue(typeof value === "string" && pattern.test(value), field)
}

// Validate approved inputs without downloads, native tools, credentials or generated files.
// The manifest is the approval boundary: well-formed new upstream digests require review
// and are verified against downloaded bytes by staging, not guessed by this offline check.
export function preflight(root = appRoot) {
  const inputs = JSON.parse(readFileSync(resolve(root, "runtime-inputs.json"), "utf8"))
  keys(inputs, ["schemaVersion", "microsandboxVersion", "libkrunfwVersion", "sourceCommit", "sourceArchiveSha256", "patches", "toolchain", "features", "libkrunfwCommit", "targets", "licenses"], "manifest fields")
  requireValue(inputs.schemaVersion === 2, "schemaVersion")
  for (const key of ["microsandboxVersion", "libkrunfwVersion", "toolchain"]) matches(inputs[key], version, key)
  for (const key of ["sourceCommit", "libkrunfwCommit"]) matches(inputs[key], revision, key)
  for (const key of ["sourceArchiveSha256"]) matches(inputs[key], digest, key)
  requireValue(inputs.features === "net,ssh,embed-binaries", "features (required net,ssh,embed-binaries capability set)")
  const patchNames = ["microsandbox-silo-network", "microsandbox-restore-policy", "microsandbox-create-stopped", "microsandbox-adopt-owned-disk", "microsandbox-log-retention-desktop-start", "microsandbox-restore-root-capacity", "microsandbox-portable-image-cache", "microsandbox-live-public-ports", "microsandbox-secret-values-stdin", "microsandbox-import-stage-id", "microsandbox-sftp-user", "microsandbox-remove-created", "microsandbox-restore-starting-control", "microsandbox-runtime-instance-id", "microsandbox-checkpoint-fs-state", "microsandbox-relay-closed-local-arena", "microsandbox-reordered-image-metadata"]
  requireValue(Array.isArray(inputs.patches) && inputs.patches.length === patchNames.length, "patches")
  for (const [index, patchInput] of inputs.patches.entries()) {
    keys(patchInput, ["path", "sha256"], `patches[${index}]`)
    requireValue(patchInput.path === `patches/${patchNames[index]}-${inputs.microsandboxVersion}.patch`, `patches[${index}].path`)
    matches(patchInput.sha256, digest, `patches[${index}].sha256`)
    const patch = realpathSync(resolve(root, patchInput.path))
    const path = relative(realpathSync(root), patch)
    requireValue(path !== ".." && !path.startsWith(`..${sep}`) && !path.startsWith(sep), `patches[${index}] containment`)
    const actual = createHash("sha256").update(readFileSync(patch)).digest("hex")
    requireValue(actual === patchInput.sha256, `patches[${index}].sha256 mismatch`)
  }
  const platforms = {
    "aarch64-apple-darwin": ["darwin", "aarch64"],
    "aarch64-unknown-linux-gnu": ["linux", "aarch64"],
    "x86_64-unknown-linux-gnu": ["linux", "x86_64"],
  }
  keys(inputs.targets, Object.keys(platforms), "supported targets")
  for (const [target, [platform, arch]] of Object.entries(platforms)) {
    const value = inputs.targets[target]
    keys(value, ["platform", "arch", "executableAsset", "executableSha256", "agentdAsset", "agentdSha256", "libraryAsset", "libraryName", "librarySha256"], `${target} fields`)
    const expected = { platform, arch, executableAsset: `msb-${platform}-${arch}`, agentdAsset: `agentd-${arch}`, libraryAsset: `libkrunfw-${platform}-${arch}.${platform === "darwin" ? "dylib" : "so"}`, libraryName: platform === "darwin" ? `libkrunfw.${inputs.libkrunfwVersion.split(".")[0]}.dylib` : `libkrunfw.so.${inputs.libkrunfwVersion}` }
    for (const [key, wanted] of Object.entries(expected)) requireValue(value[key] === wanted, `${target}.${key}`)
    for (const key of ["executableSha256", "agentdSha256", "librarySha256"]) matches(value[key], digest, `${target}.${key}`)
  }
  requireValue(inputs.targets["aarch64-apple-darwin"].agentdSha256 === inputs.targets["aarch64-unknown-linux-gnu"].agentdSha256, "shared aarch64 agentdSha256")
  const licenseSources = [
    ["microsandbox-Apache-2.0.txt", "microsandbox", inputs.sourceCommit, "LICENSE"],
    ["libkrunfw-LGPL-2.1-only.txt", "libkrunfw", inputs.libkrunfwCommit, "LICENSE-LGPL-2.1-only"],
    ["linux-GPL-2.0-only.txt", "libkrunfw", inputs.libkrunfwCommit, "LICENSE-GPL-2.0-only"],
  ]
  requireValue(Array.isArray(inputs.licenses) && inputs.licenses.length === licenseSources.length, "licenses")
  for (const [index, [name, repo, commit, file]] of licenseSources.entries()) {
    const license = inputs.licenses[index]
    keys(license, ["name", "url", "sha256"], `licenses[${index}]`)
    requireValue(license.name === name && license.url === `https://raw.githubusercontent.com/superradcompany/${repo}/${commit}/${file}`, `licenses[${index}] source`)
    matches(license.sha256, digest, `licenses[${index}].sha256`)
  }
  return inputs
}

if (process.argv[1] && existsSync(process.argv[1]) && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { preflight(); console.log("Runtime preflight passed (approved inputs and patch digest).") }
  catch (error) { console.error(error.message); process.exitCode = 1 }
}
