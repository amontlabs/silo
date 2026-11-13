import { createHash } from "node:crypto"
import { execFileSync, spawnSync } from "node:child_process"
import { access, chmod, copyFile, cp, mkdir, readFile, readdir, realpath, rename, rm, stat, writeFile } from "node:fs/promises"
import { homedir } from "node:os"
import { basename, delimiter, dirname, join, relative, resolve, sep } from "node:path"

import { constants, readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { RUNTIME_CACHE_FORMAT, cacheTemporaryRoot, fetchVerifiedFile, makeWorkFolder, ownerTag, publishEntry, removeAbandoned, runtimeCacheRoot, takePublishedEntry } from "./build-input.mjs"

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
async function hasRequiredCapabilities(executable, temporaryRoot) {
  const directory = await makeWorkFolder(temporaryRoot)
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

// Environment that changes the bytes of the release build (`cargo build --release --target
// <triple>`) of msb and the gateway: compiler flags for that target, the release profile (with
// its build-script overrides), the programs cargo runs as rustc, wrapper or linker, unstable cargo
// features, the C toolchain variables of cc-rs, the Apple SDK selection and the libclang
// selection of clang-sys and bindgen. Other profiles,
// other targets, display, network, credentials and locations are not part of it.
// https://doc.rust-lang.org/cargo/reference/environment-variables.html
function buildEnvironmentPattern(targetTriple) {
  const triple = targetTriple.toUpperCase().replaceAll("-", "_")
  return new RegExp(`^(RUSTFLAGS|CARGO_ENCODED_RUSTFLAGS|CARGO_BUILD_RUSTFLAGS|CARGO_TARGET_${triple}_(RUSTFLAGS|LINKER)|CARGO_HOST_(RUSTFLAGS|LINKER)|CARGO_PROFILE_RELEASE_(?!INCREMENTAL$).+|CARGO_UNSTABLE_.+|RUSTC|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|RUSTC_BOOTSTRAP|CARGO_BUILD_RUSTC(_WRAPPER|_WORKSPACE_WRAPPER)?|(TARGET_|HOST_)?(CC|CXX|AR|RANLIB|CFLAGS|CXXFLAGS|CPPFLAGS|ARFLAGS|LDFLAGS)(_.+)?|CRATE_CC_NO_DEFAULTS|MACOSX_DEPLOYMENT_TARGET|SDKROOT|DEVELOPER_DIR|LIBCLANG_PATH|LIBCLANG_STATIC_PATH|CLANG_PATH|LLVM_CONFIG_PATH|BINDGEN_EXTRA_CLANG_ARGS(_.+)?)$`)
}
// The variables above whose value names a program (cc-rs also takes "wrapper compiler").
const PROGRAM_VARIABLE = /^(RUSTC|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|CARGO_BUILD_RUSTC(_WRAPPER|_WORKSPACE_WRAPPER)?|CARGO_TARGET_.+_LINKER|CARGO_HOST_LINKER|(TARGET_|HOST_)?(CC|CXX|AR|RANLIB)(_.+)?)$/

// Whether the release build is incremental, as cargo decides it: CARGO_INCREMENTAL when set
// ("1" is on, anything else off), then `build.incremental`, then the release profile (off by
// default). Only the outcome is keyed, so "0" and unset agree for a release build.
function releaseIncremental(environment) {
  if (environment.CARGO_INCREMENTAL !== undefined) return environment.CARGO_INCREMENTAL === "1"
  return (environment.CARGO_BUILD_INCREMENTAL ?? environment.CARGO_PROFILE_RELEASE_INCREMENTAL ?? "false") === "true"
}

export function buildEnvironment(environment, targetTriple) {
  const pattern = buildEnvironmentPattern(targetTriple)
  return {
    ...Object.fromEntries(Object.keys(environment).filter(name => pattern.test(name)).sort().map(name => [name, environment[name]])),
    releaseIncremental: releaseIncremental(environment),
  }
}

function toolOutput(executable, args, environment) {
  // Some tools (`ld -v`) print their identity on standard error, so both streams are kept.
  const result = spawnSync(executable, args, { encoding: "utf8", env: environment, stdio: ["ignore", "pipe", "pipe"] })
  if (result.error?.code === "ENOENT") return "unavailable"
  if (result.error) throw result.error
  return `exit ${result.status}: ${result.stdout.trim()}\n${result.stderr.trim()}`
}

// A program by its resolved file and the sha256 of its contents: a command with a slash is a
// path, a bare name is looked up on the environment's PATH like a shell does.
async function programIdentity(command, environment) {
  let path = null
  if (command.includes("/")) path = resolve(command)
  else {
    for (const folder of (environment.PATH ?? "").split(delimiter)) {
      if (!folder) continue
      try {
        await access(join(folder, command), constants.X_OK)
        if ((await stat(join(folder, command))).isFile()) { path = join(folder, command); break }
      } catch {}
    }
  }
  if (!path) return { command, found: false }
  try {
    const real = await realpath(path)
    return { command, path: real, sha256: sha256(await readFile(real)) }
  } catch (error) {
    if (error.code === "ENOENT" || error.code === "ENOTDIR") return { command, found: false }
    throw error
  }
}

// The system libraries the Linux build finds through pkg-config (libdbus-sys probes dbus-1;
// the build hosts install libcap-ng with it). Their versions and flags are keyed, not
// PKG_CONFIG_PATH, which setup actions change without changing these results.
const PKG_CONFIG_LIBRARIES = ["dbus-1", "libcap-ng"]

// The C tools cargo and the build scripts find on PATH, with the flag that prints their version.
const PATH_TOOLS = [["cc", ["--version"]], ["c++", ["--version"]], ["clang", ["--version"]], ["ld", ["-v"]], ["ar", null]]

const configStrings = value => [...value.matchAll(/"((?:[^"\\]|\\.)*)"|'([^']*)'/g)].map(match => match[1] ?? match[2])

// What a cargo configuration file makes cargo read or run besides itself: `include` files and the
// `linker`, `rustc`, `rustc-wrapper` and `rustc-workspace-wrapper` programs. A line scan of the
// TOML (one string, an array or `[[include]] path`), not a full parser.
function configReferences(text) {
  const includes = []
  const programs = []
  let table = ""
  let array = null
  for (const raw of text.split("\n")) {
    const line = raw.replace(/\s#.*$|^#.*$/, "").trim()
    if (array !== null) {
      array += ` ${line}`
      if (line.includes("]")) { includes.push(...configStrings(array)); array = null }
      continue
    }
    const header = /^\[\[?\s*(.+?)\s*\]\]?$/.exec(line)
    if (header) { table = header[1]; continue }
    const pair = /^([\w."'-]+?)\s*=\s*(.*)$/.exec(line)
    if (!pair) continue
    const key = [table, pair[1].replaceAll(/["']/g, "")].filter(Boolean).join(".")
    if (key === "include" || key === "include.path") {
      if (pair[2].startsWith("[") && !pair[2].includes("]")) array = pair[2]
      else includes.push(...configStrings(pair[2]))
    } else if (/^(build|host|target\..+)\.(linker|rustc|rustc-wrapper|rustc-workspace-wrapper)$/.test(key)) {
      programs.push(...configStrings(pair[2]).slice(0, 1))
    }
  }
  return { includes, programs }
}

// The cargo configuration files that apply to a build run below `cargoDirectory`: `.cargo/config`
// and `.cargo/config.toml` in that folder and every parent, then the cargo home, each followed by
// the files it includes (relative to it). Returns their paths and digests, and the programs they
// name (a path relative to the folder above `.cargo`, or a name found on PATH).
async function cargoConfiguration(cargoDirectory, environment) {
  const folders = []
  for (let folder = resolve(cargoDirectory); ; folder = dirname(folder)) {
    folders.push(join(folder, ".cargo"))
    if (dirname(folder) === folder) break
  }
  folders.push(resolve(environment.CARGO_HOME?.trim() || join(homedir(), ".cargo")))
  const files = []
  const programs = []
  const seen = new Set()
  const read = async (path, included) => {
    if (seen.has(path)) return
    seen.add(path)
    const bytes = await readFile(path).catch(error => { if (error.code === "ENOENT" || error.code === "ENOTDIR") return null; throw error })
    if (!bytes) {
      if (included) files.push([path, "missing"])
      return
    }
    files.push([path, sha256(bytes)])
    const references = configReferences(bytes.toString("utf8"))
    for (const include of references.includes) await read(resolve(dirname(path), include), true)
    for (const program of references.programs) {
      programs.push({ source: path, ...await programIdentity(program.includes("/") ? resolve(dirname(dirname(path)), program) : program, environment) })
    }
  }
  for (const folder of [...new Set(folders)]) {
    for (const name of ["config", "config.toml"]) await read(join(folder, name), false)
  }
  return { files, programs }
}

// Identity of everything outside the pinned inputs that shapes the compiled runtime: the Apple
// SDK and developer folder, the C compiler, linker and archiver found on PATH, the cargo
// configuration files, the programs the build environment and configuration name (by content),
// the build environment and, on Linux, the pkg-config results of the system libraries.
export async function readBuildToolIdentity({ cargoDirectory, targetTriple, environment = process.env, output = toolOutput, platform = process.platform }) {
  const darwin = platform === "darwin"
  const configuration = await cargoConfiguration(cargoDirectory, environment)
  const variables = buildEnvironment(environment, targetTriple)
  const programs = []
  for (const name of Object.keys(variables).filter(name => PROGRAM_VARIABLE.test(name))) {
    for (const word of variables[name].split(/\s+/).filter(word => word && !word.startsWith("-"))) {
      programs.push({ source: name, ...await programIdentity(word, environment) })
    }
  }
  const pathTools = {}
  for (const [name, versionArgs] of PATH_TOOLS) {
    pathTools[name] = { ...await programIdentity(name, environment), version: versionArgs ? output(name, versionArgs, environment) : null }
  }
  return {
    developerDir: darwin ? output("xcode-select", ["-p"], environment) : null,
    xcode: darwin ? output("xcodebuild", ["-version"], environment) : null,
    sdk: darwin ? ["--show-sdk-path", "--show-sdk-version", "--show-sdk-build-version"].map(flag => output("xcrun", [flag], environment)) : null,
    pathTools,
    cargoConfig: configuration.files,
    programs: [...programs, ...configuration.programs],
    environment: variables,
    ...(platform === "linux" ? { pkgConfig: Object.fromEntries(PKG_CONFIG_LIBRARIES.map(library => [library, [["--modversion"], ["--cflags", "--libs"]].map(flags => output("pkg-config", [...flags, library], environment))])) } : {}),
  }
}

export function patchedBuildKey({ agentdSha256, rustcVersion, targetTriple, gatewaySourceDigest, recipeSha256, tools }) {
  return sha256(Buffer.from([
    `cache format ${RUNTIME_CACHE_FORMAT}`,
    MICROSANDBOX_SOURCE_SHA256,
    ...MICROSANDBOX_PATCHES.map(({ sha256: digest }) => digest),
    agentdSha256,
    rustcVersion,
    targetTriple,
    MICROSANDBOX_BUILD_FEATURES,
    gatewaySourceDigest,
    recipeSha256,
    JSON.stringify(tools),
  ].join("\n")))
}

// The published msb and gateway of one build key: `<cache>/patched-builds/<key>/published`.
export function patchedBuildEntry(cacheRoot, cacheKey) {
  return join(cacheRoot, "patched-builds", cacheKey, "published")
}

// The build key of msb and the gateway, and the gateway sources the build copies. Cargo runs inside
// a work folder of `temporaryRoot`, so the cargo configuration that applies there is keyed.
export async function patchedBuildInputs({ appRoot, targetTriple, agentdSha256, temporaryRoot, environment = process.env, readTools = readBuildToolIdentity }) {
  const rustcVersion = runBuildTool("rustc", [`+${MICROSANDBOX_BUILD_TOOLCHAIN}`, "--version"]).trim()
  const gatewayInputs = appRoot
    ? await readMacosGatewayInputs(appRoot)
    : null
  const gatewaySourceDigest = gatewayInputs
    ? sha256(Buffer.concat(gatewayInputs.flatMap(([name, bytes]) => [Buffer.from(`${name}\0`), bytes])))
    : ""
  const cacheKey = patchedBuildKey({
    agentdSha256,
    rustcVersion,
    targetTriple,
    gatewaySourceDigest,
    recipeSha256: sha256(await readFile(new URL(import.meta.url))),
    tools: await readTools({ cargoDirectory: temporaryRoot, targetTriple, environment }),
  })
  return { cacheKey, gatewayInputs }
}

export async function buildPatchedExecutable({
  appRoot,
  targetTriple,
  hostTriple,
  sourceArchive,
  patches,
  agentd,
  cacheRoot,
  temporaryRoot = cacheTemporaryRoot(cacheRoot),
  readTools,
  hooks,
}) {
  if (targetTriple !== hostTriple) {
    throw new Error(`Patched MicroSandbox cross-builds are not supported: host ${hostTriple}, target ${targetTriple}`)
  }
  const { cacheKey, gatewayInputs } = await patchedBuildInputs({ appRoot, targetTriple, agentdSha256: sha256(agentd), temporaryRoot, readTools })
  // The executable, the gateway and their manifest are published together as this one directory.
  const publishedRoot = patchedBuildEntry(cacheRoot, cacheKey)
  const publishedNames = gatewayInputs ? ["msb", "silo-macos-gateway"] : ["msb"]
  const taken = await takePublishedEntry(publishedRoot, publishedNames, hooks)
  if (taken && await hasRequiredCapabilities(taken.msb, temporaryRoot)) {
    return gatewayInputs
      ? { executable: taken.msb, macosGateway: taken["silo-macos-gateway"] }
      : taken.msb
  }

  // The extracted source and the cargo target live in a work folder of `temporaryRoot`, which the
  // next run removes if this process is interrupted; a failed build leaves no folder under its key.
  // Build scripts bake in CARGO_MANIFEST_DIR, so a target directory is only valid for this
  // extraction of the source; it is not kept for a retry and not shared between processes.
  const workRoot = await makeWorkFolder(temporaryRoot)
  const sourceRoot = join(workRoot, "source")
  const cargoTarget = join(workRoot, "target")
  try {
    await mkdir(sourceRoot)
    runBuildTool("/usr/bin/tar", ["-xzf", sourceArchive, "-C", sourceRoot])
    const entries = await readdir(sourceRoot, { withFileTypes: true })
    const source = entries.filter((entry) => entry.isDirectory()).map((entry) => join(sourceRoot, entry.name))
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
      accept: published => hasRequiredCapabilities(published.msb, temporaryRoot),
      temporaryRoot,
    })
    return gatewayBytes ? { executable: bytes, macosGateway: gatewayBytes } : bytes
  } finally {
    await rm(workRoot, { recursive: true, force: true })
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
  const temporaryRoot = cacheTemporaryRoot(runtimeCacheRoot(tauriRoot))
  // Staging names carry the owning process, so a run removes what an interrupted run left.
  const stagedRoot = join(runtimeRoot, `.staging-${ownerTag()}`)
  const bundledRoot = join(runtimeRoot, "microsandbox")
  const executablePath = join(binariesRoot, `msb-${targetTriple}`)
  const macosGatewayPath = join(binariesRoot, `silo-macos-gateway-${targetTriple}`)
  const libraryPath = join(stagedRoot, targetTriple, "lib", selected.libraryName)
  for (const path of [binariesRoot, runtimeRoot, stagedRoot, bundledRoot, executablePath, libraryPath]) {
    assertInside(tauriRoot, path)
  }

  const executableTemporary = `${executablePath}.tmp-${ownerTag()}`
  const gatewayTemporary = `${macosGatewayPath}.tmp-${ownerTag()}`
  await removeAbandoned(runtimeRoot, ".staging-")
  await removeAbandoned(binariesRoot, `${basename(executablePath)}.tmp-`)
  await removeAbandoned(binariesRoot, `${basename(macosGatewayPath)}.tmp-`)
  await rm(stagedRoot, { recursive: true, force: true })
  try {
    await mkdir(dirname(libraryPath), { recursive: true })

    await fetchVerifiedFile(
      fetchStream,
      `${RELEASE_BASE_URL}/${selected.executableAsset}`,
      selected.executableSha256,
      selected.executableAsset,
      join(cacheRoot, selected.executableAsset),
      { temporaryRoot },
    )
    const sourceArchive = await fetchVerifiedFile(
      fetchStream,
      sourceArtifact.url,
      sourceArtifact.sha256,
      "MicroSandbox pinned source",
      join(cacheRoot, `microsandbox-${MICROSANDBOX_COMMIT}.tar.gz`),
      { temporaryRoot },
    )
    const agentdPath = await fetchVerifiedFile(
      fetchStream,
      `${RELEASE_BASE_URL}/${selected.agentdAsset}`,
      selected.agentdSha256,
      selected.agentdAsset,
      join(cacheRoot, selected.agentdAsset),
      { temporaryRoot },
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
      temporaryRoot,
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
      { temporaryRoot },
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
        { maxBytes: 4 * 1024 * 1024, temporaryRoot },
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
