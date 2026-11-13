import { Readable } from 'node:stream'
import assert from 'node:assert/strict'
import { chmod, mkdir, mkdtemp, readFile, rm, stat, writeFile } from 'node:fs/promises'
import { spawn } from 'node:child_process'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createHash } from 'node:crypto'
import test from 'node:test'
import { publishEntry } from './build-input.mjs'
import { LFS_TRANSFER_COMMIT, LFS_TRANSFER_SOURCE_SHA256, lfsTransferBuildKey, lfsTransferGuestArchitecture, readGoCompilerLicense, stageLfsTransferRuntime } from './lfs-transfer-runtime.mjs'

const digest = bytes => createHash('sha256').update(bytes).digest('hex')
const compiler = { GOVERSION: 'go1.25.1', GOROOT: '/fixture/go', GOTOOLCHAIN: 'auto',
  GOFLAGS: '', GOEXPERIMENT: '', GOAMD64: 'v1', GOARM64: 'v8.0', GOFIPS140: 'off' }
const recipeSha256 = digest(await readFile(new URL('./lfs-transfer-runtime.mjs', import.meta.url)))
const buildIdentity = { goVersion: compiler.GOVERSION, goToolchain: compiler.GOTOOLCHAIN, recipeSha256,
  environment: { GOFLAGS: '', GOEXPERIMENT: '', GOAMD64: 'v1', GOARM64: 'v8.0', GOFIPS140: 'off' } }
const probeCompiler = async (program, args, options) => {
  assert.equal(program, 'go')
  assert.deepEqual(args, ['env', '-json', ...Object.keys(compiler)])
  assert.match(await readFile(join(options.cwd, 'go.mod'), 'utf8'), /^go 1\.25\.0$/m)
  assert.equal(options.env.GOWORK, 'off')
  assert.equal(options.env.GOOS, 'linux')
  return { stdout: JSON.stringify(compiler) }
}

async function compiledFixture(directory, identity = buildIdentity) {
  await mkdir(join(directory, 'licenses/example-module'), { recursive: true })
  const binary = Buffer.from('cached executable fixture')
  await writeFile(join(directory, 'git-lfs-transfer'), binary)
  await writeFile(join(directory, 'LICENSE-MIT.txt'), 'MIT')
  await writeFile(join(directory, 'licenses/Go-LICENSE'), 'Go')
  await writeFile(join(directory, 'licenses/example-module/LICENSE'), 'Module license')
  await writeFile(join(directory, 'manifest.json'), JSON.stringify({
    schemaVersion: identity ? 2 : 1, commit: LFS_TRANSFER_COMMIT, sourceSha256: LFS_TRANSFER_SOURCE_SHA256,
    architecture: 'arm64', binarySha256: digest(binary), licenseSha256: {
      'LICENSE-MIT.txt': digest('MIT'), 'licenses/Go-LICENSE': digest('Go'),
      'licenses/example-module/LICENSE': digest('Module license'),
    }, ...(identity ? { buildIdentity: identity } : {}),
  }))
}

const cacheBuilds = appRoot => join(appRoot, 'src-tauri/target/runtime-cache/git-lfs-transfer', LFS_TRANSFER_COMMIT, 'builds')
// The cache root's work folder, from an entry `<root>/git-lfs-transfer/<commit>/builds/<key>`.
const entryTemporaryRoot = entry => join(entry, '../../../../tmp')
const buildFiles = variant => ({
  'git-lfs-transfer': Buffer.from(`cached executable fixture${variant}`), 'LICENSE-MIT.txt': Buffer.from('MIT'),
  'licenses/Go-LICENSE': Buffer.from(`Go${variant}`), 'licenses/example-module/LICENSE': Buffer.from('Module license'),
})

// A build in the shared cache, published as the build that ran with `identity` publishes it.
async function publishedFixture(appRoot, identity = buildIdentity, files = buildFiles('')) {
  const entry = join(cacheBuilds(appRoot), lfsTransferBuildKey('arm64', identity))
  await publishEntry(entry, files, { temporaryRoot: entryTemporaryRoot(entry) })
  return entry
}

test('changed effective Go flags reject a previously compiled executable', async t => {
  const appRoot = await mkdtemp(join(tmpdir(), 'silo-lfs-effective-flags-'))
  t.after(() => rm(appRoot, { recursive: true, force: true }))
  await compiledFixture(join(appRoot, 'src-tauri/runtime/lfs-transfer'))
  await assert.rejects(stageLfsTransferRuntime({ appRoot, targetTriple: 'aarch64-apple-darwin',
    run: async (...args) => {
      await probeCompiler(...args)
      return { stdout: JSON.stringify({ ...compiler, GOFLAGS: '-tags=changed' }) }
    }, fetchStream: async () => { throw new Error('Rebuild required') } }), /Rebuild required/)
})

for (const name of ['source digest', 'binary digest']) {
  test(`matching compiler identity still rejects changed ${name}`, async t => {
    const appRoot = await mkdtemp(join(tmpdir(), 'silo-lfs-digests-'))
    t.after(() => rm(appRoot, { recursive: true, force: true }))
    const directory = join(appRoot, 'src-tauri/runtime/lfs-transfer')
    await compiledFixture(directory)
    if (name === 'binary digest') await writeFile(join(directory, 'git-lfs-transfer'), 'tampered')
    else {
      const path = join(directory, 'manifest.json')
      const manifest = JSON.parse(await readFile(path, 'utf8'))
      manifest.sourceSha256 = '0'.repeat(64)
      await writeFile(path, JSON.stringify(manifest))
    }
    await assert.rejects(stageLfsTransferRuntime({ appRoot, targetTriple: 'aarch64-apple-darwin', run: probeCompiler,
      fetchStream: async () => { throw new Error('Rebuild required') } }), /Rebuild required/)
  })
}

for (const location of ['runtime/lfs-transfer', 'shared cache']) {
  for (const [name, identity] of [
    ['legacy manifest', undefined],
    ['compiler upgrade', { ...buildIdentity, goVersion: 'go1.25.0' }],
    ['requested toolchain', { ...buildIdentity, goToolchain: 'go1.25.1' }],
    ['build recipe', { ...buildIdentity, recipeSha256: '0'.repeat(64) }],
    ['build flags', { ...buildIdentity, environment: { ...buildIdentity.environment, GOFLAGS: '-tags=old' } }],
  ]) {
    test(`${name} invalidates ${location}`, async t => {
      const appRoot = await mkdtemp(join(tmpdir(), 'silo-lfs-cache-identity-'))
      t.after(() => rm(appRoot, { recursive: true, force: true }))
      if (location !== 'shared cache') await compiledFixture(join(appRoot, 'src-tauri', location), identity ?? null)
      // A legacy shared build sat directly in builds/linux-<architecture>.
      else if (!identity) await compiledFixture(join(cacheBuilds(appRoot), 'linux-arm64'), buildIdentity)
      else await publishedFixture(appRoot, identity)
      let downloads = 0
      await assert.rejects(stageLfsTransferRuntime({ appRoot, targetTriple: 'aarch64-apple-darwin', run: probeCompiler,
        fetchStream: async () => { downloads++; return Readable.from([Buffer.from('unverified rebuild source')]) } }), /source checksum mismatch/)
      assert.equal(downloads, 1, 'Rejected executable must enter the verified source build path')
    })
  }
}

test('compiler selection changes reject both caches even when their binary digests match', async t => {
  const appRoot = await mkdtemp(join(tmpdir(), 'silo-lfs-selected-compiler-'))
  t.after(() => rm(appRoot, { recursive: true, force: true }))
  await compiledFixture(join(appRoot, 'src-tauri/runtime/lfs-transfer'))
  await publishedFixture(appRoot)
  await assert.rejects(stageLfsTransferRuntime({ appRoot, targetTriple: 'aarch64-apple-darwin',
    run: async (...args) => {
      await probeCompiler(...args)
      return { stdout: JSON.stringify({ ...compiler, GOVERSION: 'go1.25.2' }) }
    }, fetchStream: async () => { throw new Error('Rebuild required') } }), /Rebuild required/)
})

test('a published build is staged with the manifest a fresh build writes', async t => {
  const appRoot = await mkdtemp(join(tmpdir(), 'silo-lfs-published-'))
  t.after(() => rm(appRoot, { recursive: true, force: true }))
  const files = buildFiles('')
  await publishedFixture(appRoot, buildIdentity, files)
  const result = await stageLfsTransferRuntime({ appRoot, targetTriple: 'aarch64-apple-darwin', run: probeCompiler,
    fetchStream: async () => { throw new Error('A published build needs no download') } })
  for (const [name, bytes] of Object.entries(files)) assert.deepEqual(await readFile(join(result.root, name)), bytes)
  assert.deepEqual(JSON.parse(await readFile(result.manifestPath, 'utf8')), {
    schemaVersion: 2, distribution: 'charmbracelet/git-lfs-transfer', commit: LFS_TRANSFER_COMMIT, buildIdentity,
    sourceSha256: LFS_TRANSFER_SOURCE_SHA256, platform: 'linux', architecture: 'arm64', binarySha256: digest(files['git-lfs-transfer']),
    licenseSha256: { 'LICENSE-MIT.txt': digest('MIT'), 'licenses/Go-LICENSE': digest('Go'), 'licenses/example-module/LICENSE': digest('Module license') },
  })
  // The checkout's staged copy is then used as it is.
  await rm(cacheBuilds(appRoot), { recursive: true })
  await stageLfsTransferRuntime({ appRoot, targetTriple: 'aarch64-apple-darwin', run: probeCompiler,
    fetchStream: async () => { throw new Error('A staged build needs no download') } })
})

test('a published entry missing a required notice is not used', async t => {
  const appRoot = await mkdtemp(join(tmpdir(), 'silo-lfs-incomplete-'))
  t.after(() => rm(appRoot, { recursive: true, force: true }))
  const files = buildFiles('')
  delete files['licenses/Go-LICENSE']
  await publishedFixture(appRoot, buildIdentity, files)
  await assert.rejects(stageLfsTransferRuntime({ appRoot, targetTriple: 'aarch64-apple-darwin', run: probeCompiler,
    fetchStream: async () => { throw new Error('Rebuild required') } }), /Rebuild required/)
})

test('Debian Go uses the copyright notice of the package owning its compiler', async t => {
  const root = await mkdtemp(join(tmpdir(), 'silo-go-debian-license-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const compilerRoot = join(root, 'usr/lib/go-1.26')
  const docsRoot = join(root, 'usr/share/doc')
  await mkdir(join(docsRoot, 'golang-1.26-go'), { recursive: true })
  await writeFile(join(docsRoot, 'golang-1.26-go/copyright'), 'The Go Authors license notice')
  const run = async (program, args) => {
    assert.equal(program, 'dpkg-query')
    assert.deepEqual(args, ['--search', join(compilerRoot, 'bin/go')])
    return { stdout: `golang-1.26-go:amd64: ${join(compilerRoot, 'bin/go')}\n` }
  }
  assert.equal((await readGoCompilerLicense(compilerRoot, { run, docsRoot })).toString(), 'The Go Authors license notice')
})

test('official and Homebrew Go license layouts do not need a package manager', async t => {
  const root = await mkdtemp(join(tmpdir(), 'silo-go-license-layouts-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const compilerRoot = join(root, 'go/libexec')
  await mkdir(compilerRoot, { recursive: true })
  const run = async () => { throw new Error('Must not query a package manager') }
  await writeFile(join(root, 'go/LICENSE'), 'Homebrew Go license')
  assert.equal((await readGoCompilerLicense(compilerRoot, { run })).toString(), 'Homebrew Go license')
  await writeFile(join(compilerRoot, 'LICENSE'), 'Official Go license')
  assert.equal((await readGoCompilerLicense(compilerRoot, { run })).toString(), 'Official Go license')
})

test('Go license discovery refuses package output that does not own the compiler', async () => {
  await assert.rejects(readGoCompilerLicense('/nonexistent/silo-go-license', {
    run: async () => ({ stdout: '../other: /nonexistent/silo-go-license/bin/go\nother: /usr/bin/go\n' }),
  }), /No package copyright notice/)
})

test('host target selects Linux guest architecture and rejects unsupported hosts', () => {
  assert.equal(lfsTransferGuestArchitecture('aarch64-apple-darwin'), 'arm64')
  assert.equal(lfsTransferGuestArchitecture('aarch64-unknown-linux-gnu'), 'arm64')
  assert.equal(lfsTransferGuestArchitecture('x86_64-unknown-linux-gnu'), 'amd64')
  assert.throws(() => lfsTransferGuestArchitecture('../../outside'), /Unsupported/)
})

test('unverified source is rejected before any Go build can execute', async t => {
  const root = await mkdtemp(join(tmpdir(), 'silo-lfs-packaging-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  await assert.rejects(stageLfsTransferRuntime({ appRoot: root, targetTriple: 'aarch64-apple-darwin',
    run: probeCompiler,
    fetchStream: async () => Readable.from([Buffer.from('not the pinned source')]) }), /checksum mismatch/)
})


test('cached module licenses stay writable across repeated native resource copies', async t => {
  const appRoot = await mkdtemp(join(tmpdir(), 'silo-lfs-license-modes-'))
  t.after(() => rm(appRoot, { recursive: true, force: true }))
  const entry = await publishedFixture(appRoot)
  await chmod(join(entry, 'licenses/example-module/LICENSE'), 0o444)
  const options = { appRoot, targetTriple: 'aarch64-apple-darwin', run: probeCompiler,
    fetchStream: async () => { throw new Error('A valid compiled cache needs no download') } }
  const result = await stageLfsTransferRuntime(options)
  const notice = join(result.root, 'licenses/example-module/LICENSE')
  assert.equal((await stat(notice)).mode & 0o777, 0o644)
  assert.equal((await stat(result.binaryPath)).mode & 0o777, 0o755)
  await chmod(notice, 0o444)
  await stageLfsTransferRuntime(options)
  await writeFile(notice, 'Simulated repeated native resource copy')
  assert.equal((await stat(notice)).mode & 0o777, 0o644)
})


for (const location of ['runtime/lfs-transfer', 'shared cache']) {
  for (const failure of ['missing', 'changed']) {
    test(`${failure} dependency license invalidates ${location}`, async t => {
      const appRoot = await mkdtemp(join(tmpdir(), 'silo-lfs-notice-integrity-'))
      t.after(() => rm(appRoot, { recursive: true, force: true }))
      const directory = location === 'shared cache' ? await publishedFixture(appRoot) : join(appRoot, 'src-tauri', location)
      if (location !== 'shared cache') await compiledFixture(directory)
      const notice = join(directory, 'licenses/example-module/LICENSE')
      if (failure === 'missing') await rm(notice)
      else await writeFile(notice, 'Truncated license')
      await assert.rejects(stageLfsTransferRuntime({ appRoot, targetTriple: 'aarch64-apple-darwin', run: probeCompiler,
        fetchStream: async () => { throw new Error('Rebuild required for damaged notices') } }), /Rebuild required for damaged notices/)
    })
  }
}


// A second process publishes build B under the same key while a reader holds build A's manifest.
// The publisher is told A fails (as when it found a damaged entry, or checked before A appeared).
async function publishInAnotherProcess(entry, variant) {
  const files = Object.fromEntries(Object.entries(buildFiles(variant)).map(([name, bytes]) => [name, bytes.toString()]))
  const child = spawn(process.execPath, ['--input-type=module', '-e', `
import { publishEntry } from ${JSON.stringify(new URL('./build-input.mjs', import.meta.url).href)}
const files = Object.fromEntries(Object.entries(${JSON.stringify(files)}).map(([name, text]) => [name, Buffer.from(text)]))
await publishEntry(${JSON.stringify(entry)}, files, { accept: async () => false, temporaryRoot: ${JSON.stringify(entryTemporaryRoot(entry))} })
`], { stdio: ['ignore', 'ignore', 'pipe'] })
  let output = ''
  child.stderr.on('data', chunk => { output += chunk })
  const code = await new Promise(resolve => child.on('close', resolve))
  assert.equal(code, 0, output)
}

// The reader's result is a miss or exactly the build it checked; never another build or a mix.
function assertOneBuild(taken, checked) {
  if (taken !== null) assert.deepEqual(taken, checked)
}

const pausePoints = { afterManifest: 'afterManifest', 'after-git-lfs-transfer': 'afterFile' }

for (const point of Object.keys(pausePoints)) {
  test(`a reader paused at ${point} while another process publishes ends in a miss, never a mixed build`, { timeout: 30000 }, async t => {
    const appRoot = await mkdtemp(join(tmpdir(), 'silo-lfs-paused-reader-'))
    t.after(() => rm(appRoot, { recursive: true, force: true }))
    const entry = await publishedFixture(appRoot)
    let paused = false
    const pause = async () => {
      paused = true
      await publishInAnotherProcess(entry, 'B')
    }
    const hooks = point === 'afterManifest' ? { afterManifest: pause } : { afterFile: name => name === 'git-lfs-transfer' && pause() }
    await assert.rejects(stageLfsTransferRuntime({ appRoot, targetTriple: 'aarch64-apple-darwin', run: probeCompiler, hooks,
      fetchStream: async () => { throw new Error('Missed: rebuild') } }), /Missed: rebuild/)
    assert.equal(paused, true)
    // Nothing was staged from either build, and B, a valid entry, is the one published.
    await assert.rejects(stat(join(appRoot, 'src-tauri/runtime/lfs-transfer')), { code: 'ENOENT' })
    assert.equal(await readFile(join(entry, 'licenses/Go-LICENSE'), 'utf8'), 'GoB')
  })
}

// The order this cache used before: check the files in place, then read them a second time.
async function verifyThenRead(directory, { afterManifest, afterFile } = {}) {
  const manifest = JSON.parse(await readFile(join(directory, 'manifest.json'), 'utf8'))
  const names = Object.keys(manifest.files).sort()
  for (const name of names) if (digest(await readFile(join(directory, name))) !== manifest.files[name].sha256) return null
  await afterManifest?.()
  const files = {}
  for (const name of names) {
    files[name] = await readFile(join(directory, name))
    await afterFile?.(name)
  }
  return files
}

for (const point of Object.keys(pausePoints)) {
  test(`control: verifying in place then reading fails the paused reader test at ${point}`, { timeout: 30000 }, async t => {
    const appRoot = await mkdtemp(join(tmpdir(), 'silo-lfs-paused-control-'))
    t.after(() => rm(appRoot, { recursive: true, force: true }))
    const entry = await publishedFixture(appRoot)
    const pause = () => publishInAnotherProcess(entry, 'B')
    const taken = await verifyThenRead(entry, point === 'afterManifest' ? { afterManifest: pause }
      : { afterFile: name => name === 'git-lfs-transfer' && pause() })
    // It checked A and returned B's files (or A's executable with B's notices).
    assert.throws(() => assertOneBuild(taken, buildFiles('')))
    if (point === 'afterManifest') assert.deepEqual(taken, buildFiles('B'))
    else {
      assert.equal(taken['git-lfs-transfer'].toString(), 'cached executable fixture')
      assert.equal(taken['licenses/Go-LICENSE'].toString(), 'GoB')
    }
  })
}
