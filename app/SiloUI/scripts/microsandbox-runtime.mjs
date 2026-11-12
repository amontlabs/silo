import { createHash } from "node:crypto"
import { execFileSync, spawnSync } from "node:child_process"
import { chmod, copyFile, cp, mkdir, mkdtemp, readFile, readdir, rename, rm, writeFile } from "node:fs/promises"
import { homedir, tmpdir } from "node:os"
import { basename, dirname, join, relative, resolve, sep } from "node:path"

import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { fetchVerifiedFile, publishEntry, runtimeCacheRoot, takePublishedEntry } from "./build-input.mjs"

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
  // `msb restore --net-strict=false` restores a computer with a list of allowed hosts.
  "--silo-restore-strict-protocol",
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

// Runs the capability probes on the bytes taken from the cache, written to a private file, so the
// program that is checked is the program that is used.
async function hasRequiredCapabilities(executable) {
  const directory = await mkdtemp(join(tmpdir(), "silo-msb-probe-"))
  try {
    const path = join(directory, "msb")
    await writeFile(path, executable, { mode: 0o755 })
    const version = runBuildTool(path, ["--version"]).trim()
    const createHelp = runBuildTool(path, ["create", "--help"])
    const execHelp = runBuildTool(path, ["exec", "--help"])
    const sshHelp = runBuildTool(path, ["ssh", "serve", "--help"])
    return runBuildTool(path, ["snapshot", "load", "--help"]).includes("--stage-id") && sshHelp.includes("--no-start") && sshHelp.includes("--authorized-keys") && sshHelp.includes("--exit-on-stdin-close") && sshHelp.includes("--expected-machine-id") && execHelp.includes("--no-start") && execHelp.includes("--no-stdin") && hasSiloProtocolProbes(path) && version === `msb ${MICRO_SANDBOX_VERSION}` && createHelp.includes("--mount-owned") && createHelp.includes("--no-start") && createHelp.includes("--progress-json") && ["--from-sandbox", "--group", "--dest-dir", "--full", "--guest-flush", "--integrity"].every(flag => runBuildTool(path, ["snapshot", "create", "--help"]).includes(flag)) && ["--cow-mem", "--name", "--net-strict"].every(flag => runBuildTool(path, ["restore", "--help"]).includes(flag))
  } catch {
    // A verified cache entry can still be unusable; rebuild from pinned inputs.
    return false
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
}

// Environment that changes what cargo, rustc and the C build scripts produce. Every `CARGO_*`
// variable is cargo configuration, except the ones below: locations this build overrides,
// display, network and credentials, and what cargo itself sets for a build script.
const CARGO_ENVIRONMENT_IGNORED = /^CARGO(_HOME|_TARGET_DIR|_BUILD_TARGET_DIR|_BUILD_JOBS|_TERM_.*|_HTTP_.*|_NET_.*|_REGISTRY_.*|_REGISTRIES_.*|_LOG|_PKG_.*|_MANIFEST_.*|_CFG_.*|_CRATE_.*|_BIN_.*|_PRIMARY_PACKAGE|_MAKEFLAGS|_RUSTC_CURRENT_DIR|_TARGET_TMPDIR)?$/
const BUILD_ENVIRONMENT = /^(CARGO_.*|RUSTFLAGS|RUSTC|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|RUSTC_BOOTSTRAP|(TARGET_|HOST_)?(CC|CXX|AR|CFLAGS|CXXFLAGS|CPPFLAGS|ARFLAGS|LDFLAGS)(_.*)?|MACOSX_DEPLOYMENT_TARGET|SDKROOT|DEVELOPER_DIR)$/

function toolOutput(executable, args, environment) {
  // Some tools (`ld -v`) print their identity on standard error, so both streams are kept.
  const result = spawnSync(executable, args, { encoding: "utf8", env: environment, stdio: ["ignore", "pipe", "pipe"] })
  if (result.error?.code === "ENOENT") return "unavailable"
  if (result.error) throw result.error
  return `exit ${result.status}: ${result.stdout.trim()}\n${result.stderr.trim()}`
}

// The cargo configuration files that apply to a build run below `cargoDirectory`: `.cargo/config`
// and `.cargo/config.toml` in that folder and every parent, then the cargo home.
async function cargoConfigFiles(cargoDirectory, environment) {
  const folders = []
  for (let folder = resolve(cargoDirectory); ; folder = dirname(folder)) {
    folders.push(join(folder, ".cargo"))
    if (dirname(folder) === folder) break
  }
  folders.push(resolve(environment.CARGO_HOME?.trim() || join(homedir(), ".cargo")))
  const files = []
  for (const folder of [...new Set(folders)]) {
    for (const name of ["config", "config.toml"]) {
      const path = join(folder, name)
      const bytes = await readFile(path).catch(error => { if (error.code === "ENOENT" || error.code === "ENOTDIR") return null; throw error })
      if (bytes) files.push([path, sha256(bytes)])
    }
  }
  return files
}

// Identity of everything outside the pinned inputs that shapes the compiled runtime: the Apple
// SDK and developer folder, the C compiler and the linker cargo uses on this host, the cargo
// configuration files, and the build environment.
export async function readBuildToolIdentity({ cargoDirectory, environment = process.env, output = toolOutput }) {
  const darwin = process.platform === "darwin"
  return {
    developerDir: darwin ? output("xcode-select", ["-p"], environment) : null,
    xcode: darwin ? output("xcodebuild", ["-version"], environment) : null,
    sdk: darwin ? ["--show-sdk-path", "--show-sdk-version", "--show-sdk-build-version"].map(flag => output("xcrun", [flag], environment)) : null,
    cc: output("cc", ["--version"], environment),
    linker: output("ld", ["-v"], environment),
    cargoConfig: await cargoConfigFiles(cargoDirectory, environment),
    environment: Object.fromEntries(Object.keys(environment)
      .filter(name => BUILD_ENVIRONMENT.test(name) && !CARGO_ENVIRONMENT_IGNORED.test(name))
      .sort()
      .map(name => [name, environment[name]])),
  }
}

export function patchedBuildKey({ agentd, rustcVersion, targetTriple, gatewaySourceDigest, recipeSha256, tools }) {
  return sha256(Buffer.from([
    MICROSANDBOX_SOURCE_SHA256,
    ...MICROSANDBOX_PATCHES.map(({ sha256: digest }) => digest),
    sha256(agentd),
    rustcVersion,
    targetTriple,
    MICROSANDBOX_BUILD_FEATURES,
    gatewaySourceDigest,
    recipeSha256,
    JSON.stringify(tools),
  ].join("\n")))
}

export async function buildPatchedExecutable({
  appRoot,
  targetTriple,
  hostTriple,
  sourceArchive,
  patches,
  agentd,
  cacheRoot,
  readTools,
  hooks,
}) {
  if (targetTriple !== hostTriple) {
    throw new Error(`Patched MicroSandbox cross-builds are not supported: host ${hostTriple}, target ${targetTriple}`)
  }
  const rustcVersion = runBuildTool("rustc", [`+${MICROSANDBOX_BUILD_TOOLCHAIN}`, "--version"]).trim()
  const gatewayInputs = appRoot
    ? await readMacosGatewayInputs(appRoot)
    : null
  const gatewaySourceDigest = gatewayInputs
    ? sha256(Buffer.concat(gatewayInputs.flatMap(([name, bytes]) => [Buffer.from(`${name}\0`), bytes])))
    : ""
  const buildsRoot = join(cacheRoot, "patched-builds")
  const cacheKey = patchedBuildKey({
    agentd,
    rustcVersion,
    targetTriple,
    gatewaySourceDigest,
    recipeSha256: sha256(await readFile(new URL(import.meta.url))),
    tools: await (readTools ?? readBuildToolIdentity)({ cargoDirectory: buildsRoot }),
  })
  const buildRoot = join(buildsRoot, cacheKey)
  // The executable, the gateway and their manifest are published together as this one directory.
  const publishedRoot = join(buildRoot, "published")
  const publishedNames = gatewayInputs ? ["msb", "silo-macos-gateway"] : ["msb"]
  const taken = await takePublishedEntry(publishedRoot, publishedNames, hooks)
  if (taken && await hasRequiredCapabilities(taken.msb)) {
    return gatewayInputs
      ? { executable: taken.msb, macosGateway: taken["silo-macos-gateway"] }
      : taken.msb
  }

  // The work folders sit beside the keys, so a failed build leaves no folder under its key.
  await mkdir(buildsRoot, { recursive: true })
  const workRoot = await mkdtemp(join(buildsRoot, "work-"))
  // Build scripts bake in CARGO_MANIFEST_DIR, so a target directory is only valid for this
  // extraction of the source; it is not kept for a retry and not shared between processes.
  const cargoTarget = `${workRoot}-target`
  try {
    runBuildTool("/usr/bin/tar", ["-xzf", sourceArchive, "-C", workRoot])
    const entries = await import("node:fs/promises").then(({ readdir }) => readdir(workRoot, { withFileTypes: true }))
    const source = entries.filter((entry) => entry.isDirectory()).map((entry) => join(workRoot, entry.name))
    if (source.length !== 1) throw new Error("Pinned MicroSandbox source archive has an unexpected layout")
    const patchPaths = patches.map((_, index) => join(workRoot, `patch-${index}.patch`))
    await Promise.all(patches.map((patch, index) => writeFile(patchPaths[index], patch)))
    for (const patchPath of patchPaths) applyRuntimePatch(source[0], patchPath)
    if (gatewayInputs) {
      const gatewaySource = join(source[0], "macos-gateway")
      const gatewayRoot = resolve(appRoot, "src-tauri/macos-gateway")
      await cp(gatewayRoot, gatewaySource, { recursive: true, filter: path => path !== join(gatewayRoot, "target") })
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
    await publishEntry(publishedRoot, gatewayBytes ? { msb: bytes, "silo-macos-gateway": gatewayBytes } : { msb: bytes }, {
      accept: published => hasRequiredCapabilities(published.msb),
    })
    return gatewayBytes ? { executable: bytes, macosGateway: gatewayBytes } : bytes
  } finally {
    await rm(workRoot, { recursive: true, force: true })
    await rm(cargoTarget, { recursive: true, force: true })
  }
}

// Every file of the gateway crate that the build copies into the MicroSandbox checkout, in a
// fixed order, so a new module, build script or data file changes the cache key.
async function gatewayFiles(directory, prefix = "") {
  const files = []
  for (const entry of (await readdir(join(directory, prefix), { withFileTypes: true })).sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))) {
    if (prefix === "" && entry.name === "target") continue
    const name = prefix ? `${prefix}/${entry.name}` : entry.name
    if (entry.isDirectory()) files.push(...await gatewayFiles(directory, name))
    else if (entry.isFile()) files.push(name)
    else throw new Error(`The macOS gateway sources contain an unsupported file type: ${name}`)
  }
  return files
}

export async function readMacosGatewayInputs(appRoot) {
  const directory = resolve(appRoot, "src-tauri/macos-gateway")
  return Promise.all((await gatewayFiles(directory)).map(async name => [name, await readFile(join(directory, name))]))
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
  const cacheRoot = join(runtimeCacheRoot(tauriRoot), `v${MICRO_SANDBOX_VERSION}`)
  const stagedRoot = join(runtimeRoot, `.staging-${process.pid}`)
  const bundledRoot = join(runtimeRoot, "microsandbox")
  const executablePath = join(binariesRoot, `msb-${targetTriple}`)
  const macosGatewayPath = join(binariesRoot, `silo-macos-gateway-${targetTriple}`)
  const libraryPath = join(stagedRoot, targetTriple, "lib", selected.libraryName)
  for (const path of [binariesRoot, runtimeRoot, stagedRoot, bundledRoot, executablePath, libraryPath]) {
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
    const patches = await Promise.all(MICROSANDBOX_PATCHES.map(async (patchInput, index) => {
      const patchPath = resolve(appRoot, patchInput.path)
      assertInside(appRoot, patchPath)
      const patch = await readFile(patchPath)
      verifySha256(patch, patchInput.sha256, `Silo runtime patch ${index + 1}`)
      return patch
    }))
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
    if (buildExecutable === buildPatchedExecutable && !macosGateway) {
      throw new Error("Patched MicroSandbox preparation did not build the Silo gateway")
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
      if (!loadHelp.includes("--stage-id") || !["--from-sandbox", "--group", "--dest-dir", "--full", "--guest-flush", "--integrity"].every(flag => snapshotHelp.includes(flag)) || !restoreHelp.includes("--cow-mem") || !restoreHelp.includes("--name") || !restoreHelp.includes("--net-strict") || !sshHelp.includes("--no-start") || !sshHelp.includes("--authorized-keys") || !sshHelp.includes("--exit-on-stdin-close") || !sshHelp.includes("--expected-machine-id") || !execHelp.includes("--no-start") || !execHelp.includes("--no-stdin") || version !== `msb ${MICRO_SANDBOX_VERSION}` || !createHelp.includes("--mount-owned") || !createHelp.includes("--no-start") || !createHelp.includes("--progress-json")) {
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
