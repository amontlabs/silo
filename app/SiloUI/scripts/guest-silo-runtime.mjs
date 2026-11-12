import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { chmod, copyFile, mkdir, readFile, rename, rm, stat, writeFile } from 'node:fs/promises'
import { join, resolve } from 'node:path'
import { promisify } from 'node:util'
import { runtimeCacheRoot } from './build-input.mjs'
import { MICROSANDBOX_BUILD_TOOLCHAIN } from './microsandbox-runtime.mjs'

const execute = promisify(execFile)
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex')

// The Linux `silo` that Silo puts into its Linux computers (docs/SiloUI-GUEST-IMAGES.md, guest
// components): a static musl build of the CLI crate for the guest's architecture. Its C
// dependency (ring) is compiled with Zig, so a build needs Zig; the version below is the one
// the release workflows install (.github/actions/prepare-release-runtime).
export const ZIG_VERSION = '0.15.2'
export const GUEST_SILO_MANIFEST_VERSION = 1

const targets = {
  'aarch64-apple-darwin': { architecture: 'arm64', rustTarget: 'aarch64-unknown-linux-musl', zigTarget: 'aarch64-linux-musl' },
  'aarch64-unknown-linux-gnu': { architecture: 'arm64', rustTarget: 'aarch64-unknown-linux-musl', zigTarget: 'aarch64-linux-musl' },
  'x86_64-unknown-linux-gnu': { architecture: 'amd64', rustTarget: 'x86_64-unknown-linux-musl', zigTarget: 'x86_64-linux-musl' },
}

export function guestSiloTarget(targetTriple) {
  const selected = targets[targetTriple]
  if (!selected) throw new Error(`Unsupported guest silo target: ${targetTriple}`)
  return selected
}

// A build that ships (CI, or SILO_REQUIRE_GUEST_SILO=1) must have the program; a development
// build goes on without it and says so.
export function guestSiloRequired(environment) {
  return environment.SILO_REQUIRE_GUEST_SILO === '1' || environment.CI === 'true'
}

class Unavailable extends Error {}

async function zigVersion(zig, run) {
  try {
    return (await run(zig, ['version'])).stdout.trim()
  } catch {
    throw new Unavailable(`Zig is not installed (looked for "${zig}"; set SILO_ZIG to its path)`)
  }
}

// `zig cc` standing in for the C compiler and linker. Cargo passes flags zig does not take.
export const compilerWrapper = (zig, zigTarget) => `#!/bin/sh
for argument; do
  case "$argument" in
    --target=*|-Wl,--fix-cortex-a53-843419) ;;
    *) set -- "$@" "$argument" ;;
  esac
  shift
done
exec "${zig}" cc -target ${zigTarget} "$@"
`
export const archiverWrapper = zig => `#!/bin/sh
exec "${zig}" ar "$@"
`

// Cargo keys every compilation on the linker path, so the wrappers live at a path that depends only
// on their content (never a fresh temporary directory): an unchanged guest silo is not rebuilt, and
// checkouts sharing a cache root reuse each other's compiled dependencies.
async function wrapperDirectory(appRoot, files, zigVersion) {
  // The Zig release is part of the path too: cc-rs does not notice a new compiler behind the same wrapper.
  const digest = sha256(Buffer.from(JSON.stringify([files, zigVersion]))).slice(0, 16)
  const tools = join(runtimeCacheRoot(resolve(appRoot, 'src-tauri'), process.env), 'guest-cc', digest)
  await mkdir(tools, { recursive: true })
  for (const [name, content] of Object.entries(files)) {
    const temporary = join(tools, `.${name}.${process.pid}`)
    await writeFile(temporary, content, { mode: 0o755 })
    await rename(temporary, join(tools, name))
  }
  return tools
}

async function build({ appRoot, selected, zig, zigVersion, toolchain, run, environment }) {
  const tools = await wrapperDirectory(appRoot, { cc: compilerWrapper(zig, selected.zigTarget), ar: archiverWrapper(zig) }, zigVersion)
  const triple = selected.rustTarget.toUpperCase().replaceAll('-', '_')
  const targetDirectory = environment.CARGO_TARGET_DIR?.trim() || resolve(appRoot, 'src-tauri/target/guest-silo')
  await run('cargo', [...(toolchain ? [`+${toolchain}`] : []), 'build', '--release', '-p', 'silo-cli', '--target', selected.rustTarget, '--locked'], {
    cwd: appRoot,
    maxBuffer: 64 * 1024 * 1024,
    env: {
      ...environment,
      [`CC_${selected.rustTarget.replaceAll('-', '_')}`]: join(tools, 'cc'),
      [`AR_${selected.rustTarget.replaceAll('-', '_')}`]: join(tools, 'ar'),
      [`CARGO_TARGET_${triple}_LINKER`]: join(tools, 'cc'),
      [`CARGO_TARGET_${triple}_RUSTFLAGS`]: '-C link-self-contained=no',
      CARGO_TARGET_DIR: targetDirectory,
      RUSTC_WRAPPER: '',
    },
  })
  return join(targetDirectory, selected.rustTarget, 'release', 'silo')
}

async function selectToolchain(run, selected, required) {
  try {
    await run('rustc', [`+${MICROSANDBOX_BUILD_TOOLCHAIN}`, '--version'])
  } catch {
    if (required) throw new Error(`The Rust toolchain ${MICROSANDBOX_BUILD_TOOLCHAIN} is not installed`)
    return null
  }
  const installed = (await run('rustup', ['target', 'list', '--installed', '--toolchain', MICROSANDBOX_BUILD_TOOLCHAIN]).catch(() => ({ stdout: '' }))).stdout
  if (!installed.split('\n').includes(selected.rustTarget)) {
    if (required) {
      await run('rustup', ['target', 'add', '--toolchain', MICROSANDBOX_BUILD_TOOLCHAIN, selected.rustTarget])
    } else {
      throw new Unavailable(`The Rust target ${selected.rustTarget} is not installed (run: rustup target add --toolchain ${MICROSANDBOX_BUILD_TOOLCHAIN} ${selected.rustTarget})`)
    }
  }
  return MICROSANDBOX_BUILD_TOOLCHAIN
}

async function publish(appRoot, files) {
  const tauriRoot = resolve(appRoot, 'src-tauri')
  const root = join(tauriRoot, 'runtime', 'guest-silo')
  const stage = join(tauriRoot, 'runtime', `.guest-silo-staging-${process.pid}`)
  await rm(stage, { recursive: true, force: true })
  await mkdir(stage, { recursive: true })
  try {
    for (const [name, content] of Object.entries(files)) {
      if (typeof content === 'string') await writeFile(join(stage, name), content)
      else {
        await copyFile(content.from, join(stage, name))
        await chmod(join(stage, name), 0o755)
      }
    }
    await rm(root, { recursive: true, force: true })
    await rename(stage, root)
  } finally {
    await rm(stage, { recursive: true, force: true })
  }
  return { root, binaryPath: join(root, 'silo'), manifestPath: join(root, 'manifest.json') }
}

const manifestText = manifest => `${JSON.stringify(manifest, null, 2)}\n`

// Builds the guest `silo` and stages it with a manifest holding its SHA-256, under
// src-tauri/runtime/guest-silo (shipped as resources/guest-support/silo). Without Zig (or the
// Rust target) a development build stages a manifest that says so and the engine reports that
// Silo's tools cannot be installed in Linux computers; a build that ships fails.
export async function stageGuestSilo({
  appRoot,
  targetTriple,
  environment = process.env,
  run = execute,
  required = guestSiloRequired(environment),
  log = console.log,
}) {
  const selected = guestSiloTarget(targetTriple)
  const zig = environment.SILO_ZIG?.trim() || 'zig'
  let built
  try {
    const version = await zigVersion(zig, run)
    if (required && version !== ZIG_VERSION) {
      throw new Error(`Zig ${ZIG_VERSION} is required to build the Linux program of Silo's tools; found ${version}`)
    }
    const toolchain = await selectToolchain(run, selected, required)
    built = { path: await build({ appRoot, selected, zig, zigVersion: version, toolchain, run, environment }), zigVersion: version, toolchain }
  } catch (error) {
    if (required || !(error instanceof Unavailable)) {
      throw new Error(`The Linux program of Silo's tools could not be built: ${error.message}. A build that ships needs it (Zig ${ZIG_VERSION}).`, { cause: error })
    }
    const reason = `${error.message}. Silo's tools cannot be installed in Linux computers by this build.`
    log(`Guest silo: ${reason}`)
    return { available: false, reason, ...await publish(appRoot, {
      'manifest.json': manifestText({ schemaVersion: GUEST_SILO_MANIFEST_VERSION, available: false, architecture: selected.architecture, reason }),
    }) }
  }
  const bytes = await readFile(built.path)
  if ((await stat(built.path)).size === 0) throw new Error('The Linux program of Silo\'s tools is empty')
  const staged = await publish(appRoot, {
    silo: { from: built.path },
    'manifest.json': manifestText({
      schemaVersion: GUEST_SILO_MANIFEST_VERSION,
      available: true,
      platform: 'linux',
      architecture: selected.architecture,
      target: selected.rustTarget,
      binarySha256: sha256(bytes),
      size: bytes.length,
      zigVersion: built.zigVersion,
      toolchain: built.toolchain,
    }),
  })
  log(`Guest silo: built for ${selected.rustTarget} (${bytes.length} bytes, sha256 ${sha256(bytes).slice(0, 12)})`)
  return { available: true, ...staged }
}
