import assert from "node:assert/strict"
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"
import test from "node:test"

import { stageLinuxPackageTools } from "./linux-package-tools.mjs"

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..")

test("a missing Linux package input preserves the complete previous tool directory", async () => {
  const root = await mkdtemp(join(tmpdir(), "silo-linux-package-failure-"))
  try {
    const triple = "x86_64-unknown-linux-gnu"
    const binaries = join(root, "src-tauri", "binaries")
    const packageRoot = join(root, "src-tauri", "runtime", "linux-package")
    const destination = join(packageRoot, "tools")
    await mkdir(binaries, { recursive: true })
    await mkdir(destination, { recursive: true })
    const names = ["msb", "git", "git-lfs", "git-remote-http", "git-remote-https", "libkrunfw.so.5.6.1"]
    for (const name of names) await writeFile(join(destination, name), `previous-${name}`)
    // Copying the first input succeeds before the missing second input fails.
    await writeFile(join(binaries, `msb-${triple}`), "replacement-msb")

    await assert.rejects(stageLinuxPackageTools({ appRoot: root, targetTriple: triple }), { code: "ENOENT" })
    assert.deepEqual(await readdir(destination), [...names].sort())
    for (const name of names) assert.equal(await readFile(join(destination, name), "utf8"), `previous-${name}`)
    assert.deepEqual(await readdir(packageRoot), ["tools"])
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

test("failed Linux tool staging preserves the previous complete package inputs", async () => {
  const root = await mkdtemp(join(tmpdir(), "silo-linux-package-"))
  try {
    const triple = "x86_64-unknown-linux-gnu"
    const binaries = join(root, "src-tauri", "binaries")
    const packaged = join(root, "src-tauri", "runtime", "linux-package")
    const tools = join(packaged, "tools")
    await mkdir(binaries, { recursive: true })
    await mkdir(tools, { recursive: true })
    const names = ["msb", "git", "git-lfs", "git-remote-http", "git-remote-https", "libkrunfw.so.5.6.1"]
    for (const name of names) await writeFile(join(tools, name), `previous-${name}`)
    for (const name of names.slice(0, -1)) await writeFile(join(binaries, `${name}-${triple}`), `replacement-${name}`)
    // The final source is absent after the five executable copies have succeeded.
    await assert.rejects(stageLinuxPackageTools({ appRoot: root, targetTriple: triple }), { code: "ENOENT" })
    assert.deepEqual(await Promise.all(names.map(name => readFile(join(tools, name), "utf8"))),
      names.map(name => `previous-${name}`))
    assert.deepEqual(await readdir(packaged), ["tools"])
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

test("Linux package tools preserve exact staged bytes and executable modes", async () => {
  const root = await mkdtemp(join(tmpdir(), "silo-linux-package-"))
  try {
    const triple = "x86_64-unknown-linux-gnu"
    const binaries = join(root, "src-tauri", "binaries")
    const microsandbox = join(root, "src-tauri", "runtime", "microsandbox", triple, "lib")
    await mkdir(binaries, { recursive: true })
    await mkdir(microsandbox, { recursive: true })
    const sources = [
      join(binaries, `msb-${triple}`),
      join(binaries, `git-${triple}`),
      join(binaries, `git-lfs-${triple}`),
      join(binaries, `git-remote-http-${triple}`),
      join(binaries, `git-remote-https-${triple}`),
      join(microsandbox, "libkrunfw.so.5.6.1"),
    ]
    for (const [index, source] of sources.entries()) {
      await writeFile(source, `fixture-${index}`)
      await chmod(source, index < 5 ? 0o755 : 0o644)
    }

    const staged = await stageLinuxPackageTools({ appRoot: root, targetTriple: triple })
    assert.deepEqual(await Promise.all(staged.files.map(path => readFile(path, "utf8"))),
      sources.map((_, index) => `fixture-${index}`))
    assert.deepEqual(await Promise.all(staged.files.map(async path => (await stat(path)).mode & 0o111)),
      [0o111, 0o111, 0o111, 0o111, 0o111, 0])
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

test("Linux package overlay preserves non-tool resources and places tools outside scanned roots", async () => {
  const configRoot = join(appRoot, "src-tauri")
  const base = JSON.parse(await readFile(join(configRoot, "tauri.conf.json"), "utf8"))
  const linux = JSON.parse(await readFile(join(configRoot, "tauri.linux.conf.json"), "utf8"))
  const packaged = JSON.parse(await readFile(join(configRoot, "tauri.linux.package.conf.json"), "utf8"))
  assert.deepEqual(Object.keys(base.bundle.resources), [
    "runtime/microsandbox/manifest.json",
    "runtime/microsandbox/licenses/",
    "runtime/lfs-transfer/",
    "runtime/git/manifest.json",
    "runtime/git/licenses/",
    "runtime/git/share/",
    "../THIRD-PARTY-NOTICES.md",
    "runtime/release-info.json",
    "../docs/silo-help.html",
  ])
  assert.deepEqual(base.bundle.externalBin, [
    "binaries/msb", "binaries/git", "binaries/git-lfs",
    "binaries/git-remote-http", "binaries/git-remote-https",
  ])
  assert.deepEqual(linux.bundle.resources, { "runtime/git/ssl/": "git-support/ssl/" })
  assert.deepEqual(packaged.bundle.externalBin, [])
  for (const format of ["appimage", "deb", "rpm"]) {
    assert.deepEqual(packaged.bundle.linux[format].files, {
      "/usr/libexec/silo/tools": "runtime/linux-package/tools",
    })
  }
})
