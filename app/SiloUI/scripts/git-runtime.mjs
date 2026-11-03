import { execFile } from "node:child_process"
import { createHash } from "node:crypto"
import { promisify } from "node:util"
import {
  copyFile,
  chmod,
  lstat,
  mkdir,
  readFile,
  readdir,
  realpath,
  rename,
  rm,
  stat,
  writeFile,
} from "node:fs/promises"
import { basename, join, relative, resolve, sep } from "node:path"

import { fetchVerifiedFile } from "./build-input.mjs"

const execFileAsync = promisify(execFile)

export const DUGITE_RELEASE = "v2.53.0-4"
export const DUGITE_COMMIT = "4098283a7ecb8a227b9d43580336c78a06f90e5d"
export const GIT_VERSION = "2.53.0"
export const GIT_LFS_VERSION = "3.7.1"

export const packagedGitExecutables = Object.freeze({
  git: "git",
  gitLfs: "git-lfs",
  gitRemoteHttp: "git-remote-http",
  gitRemoteHttps: "git-remote-https",
})

const RELEASE_BASE_URL = `https://github.com/desktop/dugite-native/releases/download/${DUGITE_RELEASE}`
const SOURCE_BASE_URL = "https://raw.githubusercontent.com"

export const gitRuntimeTargets = Object.freeze({
  "aarch64-apple-darwin": Object.freeze({
    archive: "dugite-native-v2.53.0-4098283-macOS-arm64.tar.gz",
    sha256: "f9dc64635a5b62fbd7ad95db73268bbb8912255ac516d65d37bf7af22fcb8ffe",
    gitVersionOutput: "git version 2.53.0",
    minimumPlatform: "macOS 12.0 (Silo declares macOS 14.0)",
    needsCertificateBundle: false,
  }),
  "aarch64-unknown-linux-gnu": Object.freeze({
    archive: "dugite-native-v2.53.0-4098283-ubuntu-arm64.tar.gz",
    sha256: "a161f45af4626bb7e0c688854bd4a9aee47cc514bca404cff0a5e3536ef1c0af",
    gitVersionOutput: "git version 2.53.0.dirty",
    minimumPlatform: "glibc 2.34",
    needsCertificateBundle: true,
  }),
  "x86_64-unknown-linux-gnu": Object.freeze({
    archive: "dugite-native-v2.53.0-4098283-ubuntu-x64.tar.gz",
    sha256: "cca76aa31ad9e835e771ee7f55b73934777fbd8d16757a10d307ba06de860901",
    gitVersionOutput: "git version 2.53.0",
    minimumPlatform: "glibc 2.34",
    needsCertificateBundle: true,
  }),
})

export const gitLicenseArtifacts = Object.freeze([
  Object.freeze({
    name: "dugite-native-GPL-2.0.txt",
    url: `${SOURCE_BASE_URL}/desktop/dugite-native/${DUGITE_COMMIT}/LICENSE.md`,
    sha256: "db296f2f7f35bca3a174efb0eb392b3b17bd94b341851429a3dff411b1c2fc73",
  }),
  Object.freeze({
    name: "git-GPL-2.0.txt",
    url: `${SOURCE_BASE_URL}/git/git/67ad42147a7acc2af6074753ebd03d904476118f/COPYING`,
    sha256: "5b2198d1645f767585e8a88ac0499b04472164c0d2da22e75ecf97ef443ab32e",
  }),
  Object.freeze({
    name: "git-lfs-MIT.txt",
    url: `${SOURCE_BASE_URL}/git-lfs/git-lfs/b84b33847fe6458f36ef521534dc0eac953cb379/LICENSE.md`,
    sha256: "4fae9062ab5cdd5fb15486b728534d8aded8b4ae9d84b6d66a956f5162c366b6",
  }),
  Object.freeze({
    name: "mozilla-ca-bundle-MPL-2.0.txt",
    url: "https://www.mozilla.org/media/MPL/2.0/index.815ca599c9df.txt",
    sha256: "fab3dd6bdab226f1c08630b1dd917e11fcb4ec5e1e020e2c16f83a0a13863e85",
  }),
])

export function selectGitRuntime(targetTriple) {
  const selected = gitRuntimeTargets[targetTriple]
  if (!selected) throw new Error(`Unsupported bundled Git target: ${targetTriple}`)
  return selected
}

export function gitRuntimeSha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex")
}

export function verifyGitRuntimeSha256(bytes, expected, label) {
  const actual = gitRuntimeSha256(bytes)
  if (actual !== expected) {
    throw new Error(`${label} checksum mismatch: expected ${expected}, received ${actual}`)
  }
}

export function validateArchiveEntries(entries, label) {
  if (entries.length === 0) throw new Error(`${label} is empty`)
  for (const rawEntry of entries) {
    const entry = rawEntry.replace(/^\.\//, "").replace(/\/$/, "")
    if (!entry) continue
    const parts = entry.split("/")
    if (entry.startsWith("/") || parts.includes("..") || parts.includes("")) {
      throw new Error(`${label} contains an unsafe path: ${rawEntry}`)
    }
  }
}

function assertInside(root, candidate) {
  const nested = relative(resolve(root), resolve(candidate))
  if (nested === "" || (!nested.startsWith(`..${sep}`) && nested !== ".." && !nested.startsWith(sep))) return
  throw new Error(`Refusing to stage outside ${resolve(root)}: ${resolve(candidate)}`)
}

async function defaultExtractArchive(archiveFile, destination) {
  const { stdout } = await execFileAsync("tar", ["-tzf", archiveFile], {
    encoding: "utf8",
    maxBuffer: 4 * 1024 * 1024,
  })
  validateArchiveEntries(stdout.split("\n").filter(Boolean), basename(archiveFile))
  await execFileAsync("tar", ["-xzf", archiveFile, "-C", destination])
}

async function assertRequiredLayout(root, selected) {
  const requiredFiles = [
    "bin/git",
    "libexec/git-core/git-lfs",
    "libexec/git-core/git-remote-http",
    "libexec/git-core/git-remote-https",
  ]
  if (selected.needsCertificateBundle) requiredFiles.push("ssl/cacert.pem")

  for (const requiredFile of requiredFiles) {
    const candidate = join(root, requiredFile)
    const information = await lstat(candidate).catch(() => undefined)
    if (!information || (!information.isFile() && !information.isSymbolicLink())) {
      throw new Error(`Bundled Git archive is missing ${requiredFile}`)
    }
  }
  if (!(await stat(join(root, "share/git-core/templates"))).isDirectory()) {
    throw new Error("Bundled Git archive is missing share/git-core/templates")
  }
  for (const executable of ["bin/git", "libexec/git-core/git-lfs", "libexec/git-core/git-remote-http"]) {
    const executablePath = join(root, executable)
    if (((await stat(executablePath)).mode & 0o111) === 0) {
      throw new Error(`Bundled Git executable lost its executable mode: ${executable}`)
    }
  }
}

async function stageExternalBinaries(root, destination, targetTriple) {
  const sources = {
    git: "bin/git",
    gitLfs: "libexec/git-core/git-lfs",
    gitRemoteHttp: "libexec/git-core/git-remote-http",
    gitRemoteHttps: "libexec/git-core/git-remote-https",
  }
  await mkdir(destination, { recursive: true })
  const staged = {}
  for (const [role, source] of Object.entries(sources)) {
    const output = join(destination, `${packagedGitExecutables[role]}-${targetTriple}`)
    await writeFile(output, await readFile(join(root, source)), { mode: 0o755 })
    if (((await stat(output)).mode & 0o111) === 0) {
      throw new Error(`Bundled Git executable lost its executable mode: ${source}`)
    }
    staged[role] = output
  }
  return staged
}

async function retainClientRuntime(root, selected) {
  for (const entry of await readdir(join(root, "bin"))) {
    if (entry !== "git") await rm(join(root, "bin", entry), { recursive: true, force: true })
  }
  for (const entry of await readdir(join(root, "libexec/git-core"))) {
    if (!["git-lfs", "git-remote-http", "git-remote-https"].includes(entry)) {
      await rm(join(root, "libexec/git-core", entry), { recursive: true, force: true })
    }
  }

  const templates = join(root, ".git-templates")
  await rename(join(root, "share/git-core/templates"), templates)
  await rm(join(root, "share"), { recursive: true, force: true })
  await mkdir(join(root, "share/git-core"), { recursive: true })
  await rename(templates, join(root, "share/git-core/templates"))
  await rm(join(root, "etc"), { recursive: true, force: true })
  if (!selected.needsCertificateBundle) await rm(join(root, "ssl"), { recursive: true, force: true })
}

async function assertContainedLinks(root, directory = root) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const candidate = join(directory, entry.name)
    if (entry.isDirectory()) {
      await assertContainedLinks(root, candidate)
    } else if (entry.isSymbolicLink()) {
      const resolved = await realpath(candidate)
      assertInside(root, resolved)
    }
  }
}

export async function stageGitRuntime({
  appRoot,
  targetTriple,
  fetchStream,
  selected = selectGitRuntime(targetTriple),
  licenses = gitLicenseArtifacts,
  extractArchive = defaultExtractArchive,
}) {
  const tauriRoot = resolve(appRoot, "src-tauri")
  const runtimeRoot = join(tauriRoot, "runtime")
  const cacheRoot = join(tauriRoot, "target", "runtime-cache", "dugite", DUGITE_RELEASE)
  const stagedRoot = join(runtimeRoot, `.git-staging-${process.pid}`)
  const bundledRoot = join(runtimeRoot, "git")
  const binariesRoot = join(tauriRoot, "binaries")
  const stagedBinariesRoot = join(stagedRoot, ".external-binaries")
  const archiveFile = join(cacheRoot, selected.archive)
  for (const candidate of [runtimeRoot, cacheRoot, stagedRoot, bundledRoot, binariesRoot, stagedBinariesRoot, archiveFile]) {
    assertInside(tauriRoot, candidate)
  }

  await rm(stagedRoot, { recursive: true, force: true })
  await mkdir(stagedRoot, { recursive: true })
  try {
    await fetchVerifiedFile(
      fetchStream,
      `${RELEASE_BASE_URL}/${selected.archive}`,
      selected.sha256,
      selected.archive,
      archiveFile,
    )
    await extractArchive(archiveFile, stagedRoot)
    await retainClientRuntime(stagedRoot, selected)
    await assertRequiredLayout(stagedRoot, selected)
    await assertContainedLinks(await realpath(stagedRoot))
    const stagedBinaries = await stageExternalBinaries(stagedRoot, stagedBinariesRoot, targetTriple)

    const licensesRoot = join(stagedRoot, "licenses")
    await mkdir(licensesRoot, { recursive: true })
    for (const license of licenses) {
      const licensePath = await fetchVerifiedFile(
        fetchStream,
        license.url,
        license.sha256,
        license.name,
        join(cacheRoot, "licenses", license.name),
        { maxBytes: 4 * 1024 * 1024 },
      )
      await copyFile(licensePath, join(licensesRoot, license.name))
      await chmod(join(licensesRoot, license.name), 0o644)
    }

    const manifest = {
      schemaVersion: 1,
      targetTriple,
      distribution: "desktop/dugite-native",
      distributionRelease: DUGITE_RELEASE,
      distributionCommit: DUGITE_COMMIT,
      gitVersion: GIT_VERSION,
      gitVersionOutput: selected.gitVersionOutput,
      gitLfsVersion: GIT_LFS_VERSION,
      archive: { name: selected.archive, sha256: selected.sha256 },
      minimumPlatform: selected.minimumPlatform,
      executableSha256: {
        git: gitRuntimeSha256(await readFile(stagedBinaries.git)),
        gitLfs: gitRuntimeSha256(await readFile(stagedBinaries.gitLfs)),
        gitRemoteHttp: gitRuntimeSha256(await readFile(stagedBinaries.gitRemoteHttp)),
        gitRemoteHttps: gitRuntimeSha256(await readFile(stagedBinaries.gitRemoteHttps)),
      },
      paths: {
        git: "bin/git",
        gitExecPath: "libexec/git-core",
        gitLfs: "libexec/git-core/git-lfs",
        templates: "share/git-core/templates",
        certificateBundle: selected.needsCertificateBundle ? "ssl/cacert.pem" : null,
        packagedExecutables: {
          directory: "executableSibling",
          ...packagedGitExecutables,
        },
        packagedResourceDirectory: "git-support",
      },
    }
    await writeFile(join(stagedRoot, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`)
    await mkdir(binariesRoot, { recursive: true })
    const externalBinaries = {}
    for (const [role, stagedPath] of Object.entries(stagedBinaries)) {
      const publishedPath = join(binariesRoot, basename(stagedPath))
      await rm(publishedPath, { force: true })
      await rename(stagedPath, publishedPath)
      externalBinaries[role] = publishedPath
    }
    await rm(stagedBinariesRoot, { recursive: true, force: true })
    await rm(bundledRoot, { recursive: true, force: true })
    await rename(stagedRoot, bundledRoot)
    return {
      targetTriple,
      root: bundledRoot,
      gitPath: join(bundledRoot, "bin/git"),
      gitLfsPath: join(bundledRoot, "libexec/git-core/git-lfs"),
      manifestPath: join(bundledRoot, "manifest.json"),
      externalBinaries,
    }
  } catch (error) {
    await rm(stagedRoot, { recursive: true, force: true })
    throw error
  }
}
