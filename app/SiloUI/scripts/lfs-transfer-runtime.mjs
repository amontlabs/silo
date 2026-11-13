import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { chmod, copyFile, mkdir, mkdtemp, readdir, readFile, rename, rm, writeFile } from 'node:fs/promises'
import { dirname, join, relative, resolve } from 'node:path'
import { promisify } from 'node:util'
import { fetchVerifiedFile, publishEntry, runtimeCacheRoot, takePublishedEntry } from './build-input.mjs'
import { validateArchiveEntries } from './git-runtime.mjs'

const execute = promisify(execFile)
export const LFS_TRANSFER_COMMIT = '971c0284dc33b1ed3f7ed9dde5d4fc0cee62db6b'
export const LFS_TRANSFER_SOURCE_SHA256 = '92d6720202aa5a059c6683df78f1fa47722c0c48ff1dc4ebfc0bc8137d988702'
export const LFS_TRANSFER_SOURCE_URL = `https://codeload.github.com/charmbracelet/git-lfs-transfer/tar.gz/${LFS_TRANSFER_COMMIT}`
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex')
// Toolchain selection lines from the pinned source's go.mod; checked again on a cold build.
const goSelection = 'go 1.25.0'
const goBuildFlags = ['-mod=readonly', '-trimpath', '-buildvcs=false', '-ldflags=-s -w']
const goRecipeEnvironment = ['GOFLAGS', 'GOEXPERIMENT', 'GOAMD64', 'GOARM64', 'GOFIPS140']

export async function readGoCompilerLicense(compilerRoot, { run = execute, docsRoot = '/usr/share/doc' } = {}) {
  // Official Go puts LICENSE in GOROOT; Homebrew puts it beside libexec.
  for (const path of [join(compilerRoot, 'LICENSE'), join(dirname(compilerRoot), 'LICENSE')]) {
    try { return await readFile(path) } catch (error) {
      if (error.code !== 'ENOENT') throw error
    }
  }
  // Debian/Ubuntu relocate notices to the owning package's copyright file.
  // https://www.debian.org/doc/debian-policy/ch-docs.html#copyright-information
  const compiler = join(compilerRoot, 'bin/go')
  const owner = await run('dpkg-query', ['--search', compiler])
  for (const line of owner.stdout.trim().split('\n')) {
    const match = /^([a-z0-9][a-z0-9+.-]*)(?::[a-z0-9-]+)?: (.+)$/.exec(line)
    if (match?.[2] === compiler) return readFile(join(docsRoot, match[1], 'copyright'))
  }
  throw new Error('No package copyright notice found for the Go compiler')
}

export function lfsTransferGuestArchitecture(targetTriple) {
  if (targetTriple === 'aarch64-apple-darwin' || targetTriple === 'aarch64-unknown-linux-gnu') return 'arm64'
  if (targetTriple === 'x86_64-unknown-linux-gnu') return 'amd64'
  throw new Error(`Unsupported Git LFS transfer target: ${targetTriple}`)
}

async function licenseDigests(directory) {
  const files = ['LICENSE-MIT.txt']
  for (const entry of await readdir(join(directory, 'licenses'), { recursive: true, withFileTypes: true })) {
    if (entry.isDirectory()) continue
    if (!entry.isFile()) throw new Error('Git LFS transfer licenses contain an unsupported file type')
    files.push(relative(directory, join(entry.parentPath, entry.name)))
  }
  const entries = []
  for (const file of files.sort()) entries.push([file, sha256(await readFile(join(directory, file)))])
  return Object.fromEntries(entries)
}

async function normalizeModes(directory) {
  await chmod(directory, 0o755)
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name)
    if (entry.isDirectory()) await normalizeModes(path)
    else if (entry.isFile()) await chmod(path, entry.name === 'git-lfs-transfer' ? 0o755 : 0o644)
    else throw new Error('Git LFS transfer runtime contains an unsupported file type')
  }
}

// The shared cache holds one compiled build per source and build identity, as one entry
// (`publishEntry`): the executable, the MIT notice and the `licenses/` notices.
export function lfsTransferBuildKey(architecture, buildIdentity) {
  return `linux-${architecture}-${sha256(JSON.stringify([LFS_TRANSFER_SOURCE_SHA256, buildIdentity]))}`
}

const usableBuild = files => Boolean(files?.['git-lfs-transfer'] && files['LICENSE-MIT.txt'] && files['licenses/Go-LICENSE']) &&
  Object.keys(files).every(name => name === 'git-lfs-transfer' || name === 'LICENSE-MIT.txt' || name.startsWith('licenses/'))

const runtimeManifest = (architecture, buildIdentity, files) => `${JSON.stringify({
  schemaVersion: 2, distribution: 'charmbracelet/git-lfs-transfer', commit: LFS_TRANSFER_COMMIT, buildIdentity,
  sourceSha256: LFS_TRANSFER_SOURCE_SHA256, platform: 'linux', architecture, binarySha256: sha256(files['git-lfs-transfer']),
  licenseSha256: Object.fromEntries(Object.keys(files).filter(name => name !== 'git-lfs-transfer').sort().map(name => [name, sha256(files[name])])),
}, null, 2)}\n`

async function readBuild(directory) {
  const files = { 'git-lfs-transfer': await readFile(join(directory, 'git-lfs-transfer')) }
  for (const name of Object.keys(await licenseDigests(directory))) files[name] = await readFile(join(directory, name))
  return files
}

// Writes a build into the checkout's runtime folder beside it, then renames it into place.
async function stageBuild(tauriRoot, root, files, manifest) {
  const stage = join(tauriRoot, 'runtime', `.lfs-transfer-staging-${process.pid}`)
  await rm(stage, { recursive: true, force: true })
  try {
    for (const [name, bytes] of Object.entries(files)) {
      await mkdir(dirname(join(stage, name)), { recursive: true })
      await writeFile(join(stage, name), bytes)
    }
    await writeFile(join(stage, 'manifest.json'), manifest)
    await normalizeModes(stage)
    await rm(root, { recursive: true, force: true })
    await rename(stage, root)
  } finally {
    await rm(stage, { recursive: true, force: true })
  }
}

// The server runs inside the Linux guest, including when Silo's host is macOS.
// There is no stable upstream binary release; build the immutable source pin.
export async function stageLfsTransferRuntime({ appRoot, targetTriple, fetchStream, run = execute, hooks }) {
  const architecture = lfsTransferGuestArchitecture(targetTriple)
  const tauriRoot = resolve(appRoot, 'src-tauri')
  const root = join(tauriRoot, 'runtime', 'lfs-transfer')
  const cache = join(runtimeCacheRoot(tauriRoot), 'git-lfs-transfer', LFS_TRANSFER_COMMIT)
  const environment = { ...process.env, CGO_ENABLED: '0', GOOS: 'linux', GOARCH: architecture, GOWORK: 'off', GOSUMDB: 'sum.golang.org' }
  await mkdir(cache, { recursive: true })
  const context = await mkdtemp(join(cache, 'compiler-'))
  let compiler
  try {
    // Resolve auto/+auto in the guest module's context, not the host app's cwd.
    await writeFile(join(context, 'go.mod'), `module github.com/charmbracelet/git-lfs-transfer\n\n${goSelection}\n`)
    const result = await run('go', ['env', '-json', 'GOVERSION', 'GOROOT', 'GOTOOLCHAIN', ...goRecipeEnvironment], {
      cwd: context, env: environment,
    })
    compiler = JSON.parse(result.stdout)
    if (!/^go\d+\.\d+\.\d+(?:[-\w.]*)$/.test(compiler.GOVERSION) || !compiler.GOROOT ||
        ['GOTOOLCHAIN', ...goRecipeEnvironment].some(key => typeof compiler[key] !== 'string')) {
      throw new Error('Cannot identify the effective Go compiler and build environment')
    }
  } finally {
    await rm(context, { recursive: true, force: true })
  }
  const buildIdentity = {
    goVersion: compiler.GOVERSION, goToolchain: compiler.GOTOOLCHAIN,
    recipeSha256: sha256(await readFile(new URL(import.meta.url))),
    environment: Object.fromEntries(goRecipeEnvironment.map(key => [key, compiler[key]])),
  }
  // Use the selected compiler for the build and notices, without another auto switch.
  const go = join(compiler.GOROOT, 'bin/go')
  const buildEnvironment = { ...environment, ...buildIdentity.environment, GOTOOLCHAIN: 'local', GOROOT: compiler.GOROOT }
  const cachedBuild = join(cache, 'builds', lfsTransferBuildKey(architecture, buildIdentity))
  const binaryPath = join(root, 'git-lfs-transfer')
  const manifestPath = join(root, 'manifest.json')
  const valid = async directory => {
    try {
      const manifest = JSON.parse(await readFile(join(directory, 'manifest.json'), 'utf8'))
      await readFile(join(directory, 'LICENSE-MIT.txt'))
      await readFile(join(directory, 'licenses', 'Go-LICENSE'))
      return manifest.schemaVersion === 2 && JSON.stringify(manifest.buildIdentity) === JSON.stringify(buildIdentity) &&
        manifest.commit === LFS_TRANSFER_COMMIT && manifest.sourceSha256 === LFS_TRANSFER_SOURCE_SHA256 &&
        manifest.architecture === architecture && JSON.stringify(manifest.licenseSha256) === JSON.stringify(await licenseDigests(directory)) &&
        manifest.binarySha256 === sha256(await readFile(join(directory, 'git-lfs-transfer')))
    } catch { return false }
  }
  if (await valid(root)) {
    await normalizeModes(root)
    return { root, binaryPath, manifestPath }
  }
  // The shared build is taken against its one manifest read; the files staged are the bytes checked.
  const taken = await takePublishedEntry(cachedBuild, null, hooks)
  if (usableBuild(taken)) {
    await stageBuild(tauriRoot, root, taken, runtimeManifest(architecture, buildIdentity, taken))
    return { root, binaryPath, manifestPath }
  }

  const stage = join(tauriRoot, 'runtime', `.lfs-transfer-build-${process.pid}`)
  const archivePath = join(cache, 'source.tar.gz')
  await mkdir(cache, { recursive: true })
  await fetchVerifiedFile(fetchStream, LFS_TRANSFER_SOURCE_URL, LFS_TRANSFER_SOURCE_SHA256, 'Git LFS transfer source', archivePath)
  await rm(stage, { recursive: true, force: true })
  await mkdir(stage, { recursive: true })
  try {
    const listing = await run('tar', ['-tzf', archivePath], { maxBuffer: 1024 * 1024 })
    validateArchiveEntries(listing.stdout.trim().split('\n'), 'Git LFS transfer source')
    const source = join(cache, `source-${process.pid}`)
    await rm(source, { recursive: true, force: true })
    await mkdir(source, { recursive: true })
    try {
      await run('tar', ['-xzf', archivePath, '--strip-components=1', '-C', source])
      const selection = (await readFile(join(source, 'go.mod'), 'utf8')).split('\n').filter(line => /^(go|toolchain)\s/.test(line)).join('\n')
      if (selection !== goSelection) throw new Error('Pinned Git LFS transfer toolchain selection does not match go.mod')
      await run(go, ['build', ...goBuildFlags, '-o', join(stage, 'git-lfs-transfer'), '.'], {
        cwd: source,
        env: buildEnvironment,
        maxBuffer: 4 * 1024 * 1024,
      })
      await copyFile(join(source, 'LICENSE'), join(stage, 'LICENSE-MIT.txt'))
      const licenses = join(stage, 'licenses')
      await mkdir(licenses, { recursive: true })
      const modules = await run(go, ['list', '-mod=readonly', '-deps', '-f', '{{if .Module}}{{.Module.Path}}\t{{.Module.Dir}}{{end}}', '.'], {
        cwd: source, env: buildEnvironment,
      })
      for (const line of new Set(modules.stdout.trim().split('\n').filter(Boolean))) {
        const [module, directory] = line.split('\t')
        const notices = (await readdir(directory)).filter(name => /^(LICENSE|COPYING|NOTICE)([.-]|$)/i.test(name))
        if (notices.length === 0) throw new Error(`No license found for linked Go module ${module}`)
        const destination = join(licenses, encodeURIComponent(module))
        await mkdir(destination, { recursive: true })
        for (const notice of notices) await copyFile(join(directory, notice), join(destination, notice))
      }
      const compilerLicense = await readGoCompilerLicense(compiler.GOROOT, { run })
      await writeFile(join(licenses, 'Go-LICENSE'), compilerLicense)
    } finally {
      await rm(source, { recursive: true, force: true })
    }
    const files = await readBuild(stage)
    // An entry another checkout published meanwhile that passes the same take is kept.
    await publishEntry(cachedBuild, files, { accept: async published => usableBuild(published) })
    await stageBuild(tauriRoot, root, files, runtimeManifest(architecture, buildIdentity, files))
    return { root, binaryPath, manifestPath }
  } finally {
    await rm(stage, { recursive: true, force: true })
  }
}
