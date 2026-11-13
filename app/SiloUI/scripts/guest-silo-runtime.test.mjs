import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { chmod, mkdir, mkdtemp, readdir, readFile, rm, stat, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { execFileSync } from 'node:child_process'
import { MICROSANDBOX_BUILD_TOOLCHAIN as TOOLCHAIN } from './microsandbox-runtime.mjs'
import { ZIG_VERSION, archiverWrapper, compilerWrapper, guestSiloRequired, guestSiloTarget, stageGuestSilo } from './guest-silo-runtime.mjs'

const digest = bytes => createHash('sha256').update(bytes).digest('hex')
const quiet = () => {}

async function appRoot() {
  const root = await mkdtemp(join(tmpdir(), 'guest-silo-'))
  await mkdir(join(root, 'src-tauri/runtime'), { recursive: true })
  return root
}

// A command runner that answers like zig, rustc, rustup and cargo would; cargo writes a program.
function runner({ zig = ZIG_VERSION, rustc = true, targets = ['aarch64-unknown-linux-musl'], calls = [] } = {}) {
  return async (program, args, options) => {
    calls.push([program, args, options])
    if (program === 'zig' || program.endsWith('/zig')) {
      if (zig === null) throw Object.assign(new Error('spawn zig ENOENT'), { code: 'ENOENT' })
      return { stdout: `${zig}\n` }
    }
    if (program === 'rustc') {
      if (!rustc) throw new Error('toolchain is not installed')
      return { stdout: `rustc ${TOOLCHAIN}\n` }
    }
    if (program === 'rustup' && args[0] === 'target' && args[1] === 'list') return { stdout: `${targets.join('\n')}\n` }
    if (program === 'rustup') { targets.push(args.at(-1)); return { stdout: '' } }
    if (program === 'cargo') {
      const out = join(options.env.CARGO_TARGET_DIR, args[args.indexOf('--target') + 1], 'release')
      await mkdir(out, { recursive: true })
      await writeFile(join(out, 'silo'), 'ELF fixture program')
      return { stdout: '' }
    }
    throw new Error(`unexpected command ${program}`)
  }
}

test('each host target maps to the guest architecture of its computers', () => {
  assert.deepEqual(guestSiloTarget('aarch64-apple-darwin'), { architecture: 'arm64', rustTarget: 'aarch64-unknown-linux-musl', zigTarget: 'aarch64-linux-musl' })
  assert.equal(guestSiloTarget('aarch64-unknown-linux-gnu').architecture, 'arm64')
  assert.deepEqual(guestSiloTarget('x86_64-unknown-linux-gnu'), { architecture: 'amd64', rustTarget: 'x86_64-unknown-linux-musl', zigTarget: 'x86_64-linux-musl' })
  assert.throws(() => guestSiloTarget('x86_64-apple-darwin'), /Unsupported guest silo target/)
})

test('a build that ships is required by CI or by SILO_REQUIRE_GUEST_SILO', () => {
  assert.equal(guestSiloRequired({}), false)
  assert.equal(guestSiloRequired({ CI: 'true' }), true)
  assert.equal(guestSiloRequired({ SILO_REQUIRE_GUEST_SILO: '1' }), true)
  assert.equal(guestSiloRequired({ CI: 'false' }), false)
})

test('the program is built with zig and staged with its checksum in the manifest', async () => {
  const root = await appRoot()
  const targetDirectory = await mkdtemp(join(tmpdir(), 'guest-silo-target-'))
  const calls = []
  try {
    const staged = await stageGuestSilo({
      appRoot: root, targetTriple: 'aarch64-apple-darwin', required: false, log: quiet,
      environment: { CARGO_TARGET_DIR: targetDirectory }, run: runner({ calls }),
    })
    assert.equal(staged.available, true)
    const manifest = JSON.parse(await readFile(staged.manifestPath, 'utf8'))
    const bytes = await readFile(staged.binaryPath)
    assert.equal(manifest.binarySha256, digest(bytes))
    assert.equal(manifest.size, bytes.length)
    assert.deepEqual([manifest.schemaVersion, manifest.available, manifest.architecture, manifest.target, manifest.zigVersion],
      [1, true, 'arm64', 'aarch64-unknown-linux-musl', ZIG_VERSION])
    assert.equal((await stat(staged.binaryPath)).mode & 0o111, 0o111)
    const cargo = calls.find(([program]) => program === 'cargo')
    assert.deepEqual(cargo[1], [`+${TOOLCHAIN}`, 'build', '--release', '-p', 'silo-cli', '--target', 'aarch64-unknown-linux-musl', '--locked'])
    assert.equal(cargo[2].env.RUSTC_WRAPPER, '')
    assert.equal(cargo[2].env.CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS, '-C link-self-contained=no')
    assert.match(cargo[2].env.CC_aarch64_unknown_linux_musl, /cc$/)
  } finally {
    await rm(root, { recursive: true, force: true })
    await rm(targetDirectory, { recursive: true, force: true })
  }
})

// The guest silo's freshness is Cargo's alone: every preparation runs cargo and stages what it
// produced, and nothing of ours keeps the program between preparations or checkouts.
test('every preparation builds the guest silo with cargo and stages its output, with no check of its own', async () => {
  const root = await appRoot()
  const targetDirectory = await mkdtemp(join(tmpdir(), 'guest-silo-target-'))
  const cacheRoot = await mkdtemp(join(tmpdir(), 'guest-silo-cache-'))
  // The cache root comes from the environment passed in, not from this process's.
  const processCacheRoot = await mkdtemp(join(tmpdir(), 'guest-silo-process-cache-'))
  const previousCacheRoot = process.env.SILO_RUNTIME_CACHE_DIR
  process.env.SILO_RUNTIME_CACHE_DIR = processCacheRoot
  let builds = 0
  const run = async (program, args, options) => {
    if (program !== 'cargo') return runner()(program, args, options)
    builds++
    const out = join(options.env.CARGO_TARGET_DIR, args[args.indexOf('--target') + 1], 'release')
    await mkdir(out, { recursive: true })
    // Cargo found the build fresh on the second run and left its output as it was.
    if (builds !== 2) await writeFile(join(out, 'silo'), `ELF fixture program ${builds}`)
    return { stdout: '' }
  }
  try {
    const options = { appRoot: root, targetTriple: 'aarch64-apple-darwin', required: false, log: quiet,
      environment: { CARGO_TARGET_DIR: targetDirectory, SILO_RUNTIME_CACHE_DIR: cacheRoot }, run }
    for (const [expectedBuilds, program] of [[1, 'ELF fixture program 1'], [2, 'ELF fixture program 1'], [3, 'ELF fixture program 3']]) {
      const staged = await stageGuestSilo(options)
      assert.equal(builds, expectedBuilds, 'cargo runs on every preparation, even when nothing changed')
      assert.equal(await readFile(staged.binaryPath, 'utf8'), program)
      assert.equal(JSON.parse(await readFile(staged.manifestPath, 'utf8')).binarySha256, digest(program))
    }
    // Only the compiler wrappers live in the shared cache; the program is never kept there.
    assert.deepEqual((await readdir(cacheRoot)).sort(), ['guest-cc', 'tmp'])
    assert.deepEqual(await readdir(join(cacheRoot, 'tmp')), [])
    assert.deepEqual(await readdir(processCacheRoot), [])
  } finally {
    if (previousCacheRoot === undefined) delete process.env.SILO_RUNTIME_CACHE_DIR
    else process.env.SILO_RUNTIME_CACHE_DIR = previousCacheRoot
    await rm(root, { recursive: true, force: true })
    await rm(targetDirectory, { recursive: true, force: true })
    await rm(cacheRoot, { recursive: true, force: true })
    await rm(processCacheRoot, { recursive: true, force: true })
  }
})

test('a development build without zig goes on and stages a manifest that says why', async () => {
  const root = await appRoot()
  const messages = []
  try {
    const staged = await stageGuestSilo({
      appRoot: root, targetTriple: 'x86_64-unknown-linux-gnu', required: false, environment: {},
      log: message => messages.push(message), run: runner({ zig: null }),
    })
    assert.equal(staged.available, false)
    assert.match(staged.reason, /Zig is not installed/)
    assert.match(staged.reason, /Silo's tools cannot be installed in Linux computers by this build/)
    assert.match(messages.join('\n'), /cannot be installed in Linux computers/)
    const manifest = JSON.parse(await readFile(staged.manifestPath, 'utf8'))
    assert.deepEqual([manifest.available, manifest.architecture], [false, 'amd64'])
    await assert.rejects(stat(staged.binaryPath))
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

test('a development build without the Rust musl target says how to add it', async () => {
  const root = await appRoot()
  try {
    const staged = await stageGuestSilo({ appRoot: root, targetTriple: 'aarch64-apple-darwin', required: false, environment: {}, log: quiet, run: runner({ targets: [] }) })
    assert.equal(staged.available, false)
    assert.match(staged.reason, new RegExp(`rustup target add --toolchain ${TOOLCHAIN.replaceAll('.', '\\.')} aarch64-unknown-linux-musl`))
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

test('a build that ships fails without zig, with the wrong zig, or when the build fails', async () => {
  const root = await appRoot()
  try {
    await assert.rejects(stageGuestSilo({ appRoot: root, targetTriple: 'aarch64-apple-darwin', required: true, environment: {}, log: quiet, run: runner({ zig: null }) }),
      /could not be built: Zig is not installed.*A build that ships needs it \(Zig 0\.15\.2\)/)
    await assert.rejects(stageGuestSilo({ appRoot: root, targetTriple: 'aarch64-apple-darwin', required: true, environment: {}, log: quiet, run: runner({ zig: '0.13.0' }) }),
      /Zig 0\.15\.2 is required.*found 0\.13\.0/)
    const failing = async (program, args, options) => {
      if (program === 'cargo') throw new Error('linker failed')
      return runner()(program, args, options)
    }
    await assert.rejects(stageGuestSilo({ appRoot: root, targetTriple: 'aarch64-apple-darwin', required: true, environment: {}, log: quiet, run: failing }), /linker failed/)
    await assert.rejects(stat(join(root, 'src-tauri/runtime/guest-silo/silo')))
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

test('a build that ships adds the Rust target itself', async () => {
  const root = await appRoot()
  const targetDirectory = await mkdtemp(join(tmpdir(), 'guest-silo-target-'))
  const calls = []
  try {
    const staged = await stageGuestSilo({ appRoot: root, targetTriple: 'aarch64-apple-darwin', required: true, log: quiet,
      environment: { CARGO_TARGET_DIR: targetDirectory }, run: runner({ targets: [], calls }) })
    assert.equal(staged.available, true)
    assert.ok(calls.some(([program, args]) => program === 'rustup' && args.join(' ') === `target add --toolchain ${TOOLCHAIN} aarch64-unknown-linux-musl`))
  } finally {
    await rm(root, { recursive: true, force: true })
    await rm(targetDirectory, { recursive: true, force: true })
  }
})

test('the compiler wrapper drops the flags zig does not take and keeps the rest', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'guest-silo-cc-'))
  try {
    await writeFile(join(dir, 'zig'), '#!/bin/sh\nprintf "%s\\n" "$@"\n')
    await chmod(join(dir, 'zig'), 0o755)
    await writeFile(join(dir, 'cc'), compilerWrapper(join(dir, 'zig'), 'aarch64-linux-musl'), { mode: 0o755 })
    const out = execFileSync(join(dir, 'cc'), ['--target=aarch64-unknown-linux-musl', '-c', 'a b.c', '-Wl,--fix-cortex-a53-843419', '-o', 'x'], { encoding: 'utf8' })
    assert.equal(out, 'cc\n-target\naarch64-linux-musl\n-c\na b.c\n-o\nx\n')
    await writeFile(join(dir, 'ar'), archiverWrapper(join(dir, 'zig')), { mode: 0o755 })
    assert.equal(execFileSync(join(dir, 'ar'), ['rcs', 'lib.a'], { encoding: 'utf8' }), 'ar\nrcs\nlib.a\n')
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
})

test('ZIG_VERSION matches the pinned version in .github/actions/prepare-release-runtime/action.yml', async () => {
  const actionYaml = await readFile(join(import.meta.dirname, '../../../.github/actions/prepare-release-runtime/action.yml'), 'utf8')
  const match = actionYaml.match(/ZIG_VERSION:\s*([0-9.]+)/)
  assert.ok(match, 'ZIG_VERSION found in action.yml')
  assert.equal(match[1], ZIG_VERSION)
})
