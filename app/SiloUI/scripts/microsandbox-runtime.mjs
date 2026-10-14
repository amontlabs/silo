import { createHash } from "node:crypto"
import { execFileSync } from "node:child_process"
import { chmod, copyFile, cp, mkdir, mkdtemp, readFile, rename, rm, writeFile } from "node:fs/promises"
import { basename, dirname, join, relative, resolve, sep } from "node:path"

import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { fetchVerifiedFile, isVerifiedFile } from "./build-input.mjs"

const inputs = JSON.parse(readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), "../runtime-inputs.json"), "utf8"))
export const MICRO_SANDBOX_VERSION = inputs.microsandboxVersion
export const LIBKRUNFW_VERSION = inputs.libkrunfwVersion
export const RELEASE_BASE_URL = `https://github.com/superradcompany/microsandbox/releases/download/v${MICRO_SANDBOX_VERSION}`
const MICROSANDBOX_COMMIT = inputs.sourceCommit
export const MICROSANDBOX_SOURCE_URL = `https://codeload.github.com/superradcompany/microsandbox/tar.gz/${MICROSANDBOX_COMMIT}`
export const MICROSANDBOX_SOURCE_SHA256 = inputs.sourceArchiveSha256
export const MICROSANDBOX_PATCHES = Object.freeze(inputs.patches.map(Object.freeze))
export const MICROSANDBOX_BUILD_TOOLCHAIN = inputs.toolchain
export const MICROSANDBOX_BUILD_FEATURES = inputs.features
export const runtimeTargets = Object.freeze(Object.fromEntries(Object.entries(inputs.targets).map(([target, value]) => [target, Object.freeze(value)])))
export const licenseArtifacts = Object.freeze(inputs.licenses.map(Object.freeze))

export function resolveRuntimeTarget(environment, hostTriple) {
  const explicitTarget = environment.SILO_RUNTIME_TARGET?.trim()
  if (explicitTarget) return explicitTarget
  const tauriTarget = environment.TAURI_ENV_TARGET_TRIPLE?.trim()
  if (tauriTarget) return tauriTarget
  return hostTriple().trim()
}

export function selectRuntime(targetTriple) {
  const selected = runtimeTargets[targetTriple]
  if (!selected) {
    throw new Error(`Unsupported bundled MicroSandbox target: ${targetTriple}`)
  }
  return selected
}

export function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex")
}

export function verifySha256(bytes, expected, label) {
  const actual = sha256(bytes)
  if (actual !== expected) {
    throw new Error(`${label} checksum mismatch: expected ${expected}, received ${actual}`)
  }
}

function assertInside(root, candidate) {
  const path = relative(resolve(root), resolve(candidate))
  if (path === "" || (!path.startsWith(`..${sep}`) && path !== ".." && !path.startsWith(sep))) return
  throw new Error(`Refusing to stage outside ${resolve(root)}: ${resolve(candidate)}`)
}

function runBuildTool(executable, args, options = {}) {
  return execFileSync(executable, args, {
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
    stdio: ["ignore", "pipe", "inherit"],
    ...options,
  })
}

const SILO_PROTOCOL_PROBES = [
  "--silo-storage-protocol",
  "--silo-desktop-protocol",
  "--silo-github-token-protocol",
  "--silo-github-protocol",
  "--silo-working-account-protocol",
  "--silo-secret-values-protocol",
  // `msb inspect --format json` reports `runtime_instance_id` for a running sandbox.
  // Computer-use setup and storage reclaim refuse to act without it.
  "--silo-runtime-instance-protocol",
]

function hasSiloProtocolProbes(executable) {
  return SILO_PROTOCOL_PROBES.every(probe => runBuildTool(executable, [probe]).trim() === "1")
}

export function applyRuntimePatch(sourceRoot, patchPath) {
  // Extracted sources live below Silo's checkout. Give Git a local root;
  // otherwise `git apply` can silently skip every path as outside the cwd.
  runBuildTool("/usr/bin/git", ["init", "--quiet"], { cwd: sourceRoot })
  runBuildTool("/usr/bin/git", ["apply", "--check", patchPath], { cwd: sourceRoot })
  runBuildTool("/usr/bin/git", ["apply", patchPath], { cwd: sourceRoot })
}

export async function buildPatchedExecutable({
  appRoot,
  targetTriple,
  hostTriple,
  sourceArchive,
  patches,
  agentd,
  cacheRoot,
}) {
  if (targetTriple !== hostTriple) {
    throw new Error(`Patched MicroSandbox cross-builds are not supported: host ${hostTriple}, target ${targetTriple}`)
  }
  const rustcVersion = runBuildTool("rustc", [`+${MICROSANDBOX_BUILD_TOOLCHAIN}`, "--version"]).trim()
  const targetEnvironment = targetTriple.toUpperCase().replaceAll("-", "_")
  const compilerFlags = Object.fromEntries([
    "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS",
    `CARGO_TARGET_${targetEnvironment}_RUSTFLAGS`,
    ...Object.keys(process.env).filter(name => name.startsWith("CARGO_PROFILE_RELEASE_")).sort(),
  ].map(name => [name, process.env[name] ?? null]))
  const gatewayInputs = targetTriple === "aarch64-apple-darwin" && appRoot
    ? await readMacosGatewayInputs(appRoot)
    : null
  const gatewaySourceDigest = gatewayInputs
    ? sha256(Buffer.concat(gatewayInputs.flatMap(([name, bytes]) => [Buffer.from(`${name}\0`), bytes])))
    : ""
  const cacheKey = sha256(Buffer.from([
    MICROSANDBOX_SOURCE_SHA256,
    ...MICROSANDBOX_PATCHES.map(({ sha256: digest }) => digest),
    sha256(agentd),
    rustcVersion,
    targetTriple,
    MICROSANDBOX_BUILD_FEATURES,
    gatewaySourceDigest,
    sha256(await readFile(new URL(import.meta.url))),
    JSON.stringify(compilerFlags),
  ].join("\n")))
  const buildRoot = join(cacheRoot, "patched-builds", cacheKey)
  const cachedExecutable = join(buildRoot, "msb")
  const cachedDigest = join(buildRoot, "msb.sha256")
  const cachedGateway = join(buildRoot, "silo-macos-gateway")
  const cachedGatewayDigest = join(buildRoot, "silo-macos-gateway.sha256")
  const cachedGatewayValid = !gatewayInputs || await isVerifiedFile(
    cachedGateway,
    (await readFile(cachedGatewayDigest, "utf8").catch(() => "")).trim(),
    "Compiled Silo macOS gateway",
  )
  if (cachedGatewayValid && await isVerifiedFile(cachedExecutable, (await readFile(cachedDigest, "utf8").catch(() => "")).trim(), "Compiled MicroSandbox")) {
    try {
      const version = runBuildTool(cachedExecutable, ["--version"]).trim()
      const createHelp = runBuildTool(cachedExecutable, ["create", "--help"])
      const execHelp = runBuildTool(cachedExecutable, ["exec", "--help"])
      const sshHelp = runBuildTool(cachedExecutable, ["ssh", "serve", "--help"])
      if (runBuildTool(cachedExecutable, ["snapshot", "load", "--help"]).includes("--stage-id") && sshHelp.includes("--no-start") && sshHelp.includes("--authorized-keys") && sshHelp.includes("--exit-on-stdin-close") && sshHelp.includes("--expected-machine-id") && execHelp.includes("--no-start") && execHelp.includes("--no-stdin") && hasSiloProtocolProbes(cachedExecutable) && version === `msb ${MICRO_SANDBOX_VERSION}` && createHelp.includes("--mount-owned") && createHelp.includes("--no-start") && createHelp.includes("--progress-json") && ["--from-sandbox", "--group", "--dest-dir", "--full", "--guest-flush", "--integrity"].every(flag => runBuildTool(cachedExecutable, ["snapshot", "create", "--help"]).includes(flag)) && ["--cow-mem", "--name"].every(flag => runBuildTool(cachedExecutable, ["restore", "--help"]).includes(flag))) {
        const executable = await readFile(cachedExecutable)
        return gatewayInputs
          ? { executable, macosGateway: await readFile(cachedGateway) }
          : executable
      }
    } catch {
      // A verified cache file can still be unusable; rebuild from pinned inputs.
    }
  }

  await mkdir(buildRoot, { recursive: true })
  const workRoot = await mkdtemp(join(buildRoot, "work-"))
  const cargoTarget = join(buildRoot, "cargo-target")
  try {
    runBuildTool("/usr/bin/tar", ["-xzf", sourceArchive, "-C", workRoot])
    const entries = await import("node:fs/promises").then(({ readdir }) => readdir(workRoot, { withFileTypes: true }))
    const source = entries.filter((entry) => entry.isDirectory()).map((entry) => join(workRoot, entry.name))
    if (source.length !== 1) throw new Error("Pinned MicroSandbox source archive has an unexpected layout")
    for (let index = 0; index < patches.length; index += 1) {
      const patchPath = join(workRoot, `patch-${index}.patch`)
      await writeFile(patchPath, patches[index])
      applyRuntimePatch(source[0], patchPath)
    }
    if (gatewayInputs) {
      const gatewaySource = join(source[0], "macos-gateway")
      await cp(resolve(appRoot, "src-tauri/macos-gateway"), gatewaySource, { recursive: true })
    }
    runBuildTool("cargo", [`+${MICROSANDBOX_BUILD_TOOLCHAIN}`, "fetch", "--locked", "--target", targetTriple], { cwd: source[0] })
    const agentdPath = join(source[0], "build", "agentd")
    await mkdir(dirname(agentdPath), { recursive: true })
    await writeFile(agentdPath, agentd, { mode: 0o755 })
    await chmod(agentdPath, 0o755)
    runBuildTool("cargo", [
      `+${MICROSANDBOX_BUILD_TOOLCHAIN}`,
      "build",
      "--locked",
      "--release",
      "--no-default-features",
      "--features",
      MICROSANDBOX_BUILD_FEATURES,
      "--target",
      targetTriple,
      "-p",
      "microsandbox-cli",
    ], { cwd: source[0], env: { ...process.env, CARGO_TARGET_DIR: cargoTarget } })
    const built = join(cargoTarget, targetTriple, "release", "msb")
    const managedSshHelp = runBuildTool(built, ["ssh", "serve", "--help"])
    if (!managedSshHelp.includes("--authorized-keys") || !managedSshHelp.includes("--exit-on-stdin-close") || !managedSshHelp.includes("--expected-machine-id")) {
      throw new Error("The built MicroSandbox is missing managed SSH access support")
    }
    if (!hasSiloProtocolProbes(built)) {
      throw new Error("The built MicroSandbox is missing one or more required Silo protocol boundaries")
    }
    if (!runBuildTool(built, ["snapshot", "load", "--help"]).includes("--stage-id")) {
      throw new Error("The built MicroSandbox is missing operation-owned snapshot staging")
    }
    const bytes = await readFile(built)
    let gatewayBytes
    if (gatewayInputs) {
      runBuildTool("cargo", [
        `+${MICROSANDBOX_BUILD_TOOLCHAIN}`,
        "build",
        "--locked",
        "--release",
        "--target",
        targetTriple,
        "--manifest-path",
        join(source[0], "macos-gateway/Cargo.toml"),
        "--bin",
        "silo-macos-gateway",
      ], { cwd: source[0], env: { ...process.env, CARGO_TARGET_DIR: cargoTarget } })
      gatewayBytes = await readFile(join(cargoTarget, targetTriple, "release/silo-macos-gateway"))
    }
    await mkdir(buildRoot, { recursive: true })
    await writeFile(`${cachedExecutable}.tmp-${process.pid}`, bytes, { mode: 0o755 })
    await rename(`${cachedExecutable}.tmp-${process.pid}`, cachedExecutable)
    await writeFile(cachedDigest, `${sha256(bytes)}\n`)
    if (gatewayBytes) {
      const gatewayTemporary = `${cachedGateway}.tmp-${process.pid}`
      await writeFile(gatewayTemporary, gatewayBytes, { mode: 0o755 })
      await chmod(gatewayTemporary, 0o755)
      await rename(gatewayTemporary, cachedGateway)
      await writeFile(cachedGatewayDigest, `${sha256(gatewayBytes)}\n`)
      return { executable: bytes, macosGateway: gatewayBytes }
    }
    return bytes
  } finally {
    await rm(workRoot, { recursive: true, force: true })
  }
}

async function readMacosGatewayInputs(appRoot) {
  const directory = resolve(appRoot, "src-tauri/macos-gateway")
  const names = [
    "Cargo.toml",
    "Cargo.lock",
    "reserved-secret-names.json",
    "src/control.rs",
    "src/frames.rs",
    "src/main.rs",
    "src/network.rs",
  ]
  return Promise.all(names.map(async name => [name, await readFile(join(directory, name))]))
}

export async function stageRuntime({
  appRoot,
  targetTriple,
  hostTriple = targetTriple,
  fetchStream,
  selected = selectRuntime(targetTriple),
  licenses = licenseArtifacts,
  sourceArtifact = { url: MICROSANDBOX_SOURCE_URL, sha256: MICROSANDBOX_SOURCE_SHA256 },
  buildExecutable = buildPatchedExecutable,
  verifyExecutable = true,
}) {
  const tauriRoot = resolve(appRoot, "src-tauri")
  const binariesRoot = join(tauriRoot, "binaries")
  const runtimeRoot = join(tauriRoot, "runtime")
  const cacheRoot = join(tauriRoot, "target", "runtime-cache", `v${MICRO_SANDBOX_VERSION}`)
  const stagedRoot = join(runtimeRoot, `.staging-${process.pid}`)
  const bundledRoot = join(runtimeRoot, "microsandbox")
  const executablePath = join(binariesRoot, `msb-${targetTriple}`)
  const macosGatewayPath = join(binariesRoot, `silo-macos-gateway-${targetTriple}`)
  const libraryPath = join(stagedRoot, targetTriple, "lib", selected.libraryName)
  for (const path of [binariesRoot, runtimeRoot, cacheRoot, stagedRoot, bundledRoot, executablePath, libraryPath]) {
    assertInside(tauriRoot, path)
  }

  const executableTemporary = `${executablePath}.tmp-${process.pid}`
  const gatewayTemporary = `${macosGatewayPath}.tmp-${process.pid}`
  await rm(stagedRoot, { recursive: true, force: true })
  try {
    await mkdir(dirname(libraryPath), { recursive: true })

    await fetchVerifiedFile(
      fetchStream,
      `${RELEASE_BASE_URL}/${selected.executableAsset}`,
      selected.executableSha256,
      selected.executableAsset,
      join(cacheRoot, selected.executableAsset),
    )
    const sourceArchive = await fetchVerifiedFile(
      fetchStream,
      sourceArtifact.url,
      sourceArtifact.sha256,
      "MicroSandbox pinned source",
      join(cacheRoot, `microsandbox-${MICROSANDBOX_COMMIT}.tar.gz`),
    )
    const agentdPath = await fetchVerifiedFile(
      fetchStream,
      `${RELEASE_BASE_URL}/${selected.agentdAsset}`,
      selected.agentdSha256,
      selected.agentdAsset,
      join(cacheRoot, selected.agentdAsset),
    )
    const patches = []
    for (const [index, patchInput] of MICROSANDBOX_PATCHES.entries()) {
      const patchPath = resolve(appRoot, patchInput.path)
      assertInside(appRoot, patchPath)
      const patch = await readFile(patchPath)
      verifySha256(patch, patchInput.sha256, `Silo runtime patch ${index + 1}`)
      patches.push(patch)
    }
    const buildOutput = await buildExecutable({
      appRoot,
      targetTriple,
      hostTriple,
      sourceArchive,
      patches,
      agentd: await readFile(agentdPath),
      cacheRoot,
    })
    const outputs = buildOutput && !ArrayBuffer.isView(buildOutput)
      ? buildOutput
      : { executable: buildOutput }
    const executable = Buffer.from(outputs.executable)
    const macosGateway = outputs.macosGateway ? Buffer.from(outputs.macosGateway) : null
    if (targetTriple === "aarch64-apple-darwin" && buildExecutable === buildPatchedExecutable && !macosGateway) {
      throw new Error("Patched MicroSandbox preparation did not build the Silo macOS gateway")
    }
    const library = await fetchVerifiedFile(
      fetchStream,
      `${RELEASE_BASE_URL}/${selected.libraryAsset}`,
      selected.librarySha256,
      selected.libraryAsset,
      join(cacheRoot, selected.libraryAsset),
    )

    await mkdir(binariesRoot, { recursive: true })
    await writeFile(executableTemporary, executable, { mode: 0o755 })
    await chmod(executableTemporary, 0o755)
    if (macosGateway) {
      await writeFile(gatewayTemporary, macosGateway, { mode: 0o755 })
      await chmod(gatewayTemporary, 0o755)
    }
    if (verifyExecutable) {
      const isolatedHome = join(stagedRoot, "verify-home")
      await mkdir(isolatedHome, { recursive: true })
      const environment = {
        ...process.env,
        HOME: isolatedHome,
        MSB_HOME: isolatedHome,
        MSB_PATH: executableTemporary,
        MSB_LIBKRUNFW_PATH: libraryPath,
      }
      const version = runBuildTool(executableTemporary, ["--version"], { env: environment }).trim()
      const createHelp = runBuildTool(executableTemporary, ["create", "--help"], { env: environment })
      const execHelp = runBuildTool(executableTemporary, ["exec", "--help"], { env: environment })
      const sshHelp = runBuildTool(executableTemporary, ["ssh", "serve", "--help"], { env: environment })
      const snapshotHelp = runBuildTool(executableTemporary, ["snapshot", "create", "--help"])
      const loadHelp = runBuildTool(executableTemporary, ["snapshot", "load", "--help"])
      const restoreHelp = runBuildTool(executableTemporary, ["restore", "--help"])
      if (!loadHelp.includes("--stage-id") || !["--from-sandbox", "--group", "--dest-dir", "--full", "--guest-flush", "--integrity"].every(flag => snapshotHelp.includes(flag)) || !restoreHelp.includes("--cow-mem") || !restoreHelp.includes("--name") || !sshHelp.includes("--no-start") || !sshHelp.includes("--authorized-keys") || !sshHelp.includes("--exit-on-stdin-close") || !sshHelp.includes("--expected-machine-id") || !execHelp.includes("--no-start") || !execHelp.includes("--no-stdin") || version !== `msb ${MICRO_SANDBOX_VERSION}` || !createHelp.includes("--mount-owned") || !createHelp.includes("--no-start") || !createHelp.includes("--progress-json")) {
        throw new Error("Patched MicroSandbox executable failed its version, stopped-create, or managed SSH capability check")
      }
      await rm(isolatedHome, { recursive: true, force: true })
    }
    await copyFile(library, libraryPath)
    await chmod(libraryPath, 0o644)

    const licensesRoot = join(stagedRoot, "licenses")
    await mkdir(licensesRoot, { recursive: true })
    for (const license of licenses) {
      const licensePath = await fetchVerifiedFile(
        fetchStream,
        license.url,
        license.sha256,
        license.name,
        join(cacheRoot, "licenses", basename(license.url)),
        { maxBytes: 4 * 1024 * 1024 },
      )
      await copyFile(licensePath, join(licensesRoot, license.name))
      await chmod(join(licensesRoot, license.name), 0o644)
    }

    const manifest = {
      schemaVersion: 2,
      microsandboxVersion: MICRO_SANDBOX_VERSION,
      libkrunfwVersion: LIBKRUNFW_VERSION,
      targetTriple,
      executable: {
        bundledName: "msb",
        sha256: sha256(executable),
        sourceCommit: MICROSANDBOX_COMMIT,
        sourceArchiveSha256: MICROSANDBOX_SOURCE_SHA256,
        patchSha256s: MICROSANDBOX_PATCHES.map(({ sha256: digest }) => digest),
        toolchain: MICROSANDBOX_BUILD_TOOLCHAIN,
        features: MICROSANDBOX_BUILD_FEATURES,
        officialReleaseAsset: selected.executableAsset,
        officialReleaseSha256: selected.executableSha256,
        embeddedAgentdReleaseAsset: selected.agentdAsset,
        embeddedAgentdReleaseSha256: selected.agentdSha256,
      },
      library: {
        bundledName: selected.libraryName,
        releaseAsset: selected.libraryAsset,
        sha256: selected.librarySha256,
      },
    }
    await writeFile(join(stagedRoot, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`)
    await rename(executableTemporary, executablePath)
    if (macosGateway) await rename(gatewayTemporary, macosGatewayPath)
    await rm(bundledRoot, { recursive: true, force: true })
    await rename(stagedRoot, bundledRoot)

    return {
      targetTriple,
      executablePath,
      gatewayPath: macosGateway ? macosGatewayPath : null,
      libraryPath: join(bundledRoot, targetTriple, "lib", selected.libraryName),
      manifestPath: join(bundledRoot, "manifest.json"),
    }
  } finally {
    await Promise.all([
      rm(executableTemporary, { force: true }),
      rm(gatewayTemporary, { force: true }),
      rm(stagedRoot, { recursive: true, force: true }),
    ])
  }
}
