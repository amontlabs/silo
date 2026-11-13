import { Readable } from "node:stream"
import { execFileSync, spawnSync, type ExecFileSyncOptions, type SpawnSyncOptions } from "node:child_process"
import { mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { cp, mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"

import { describe, expect, it, vi } from "vitest"
import {
  applyRuntimePatch,
  runtimeTargets,
  MICROSANDBOX_PATCHES,
  MICRO_SANDBOX_VERSION,
  MICROSANDBOX_BUILD_TOOLCHAIN,
  resolveRuntimeTarget,
  selectRuntime,
  sha256,
  stageRuntime,
  verifySha256,
} from "../../scripts/microsandbox-runtime.mjs"

vi.mock("node:child_process", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:child_process")>()
  return { ...actual, execFileSync: vi.fn(actual.execFileSync), spawnSync: vi.fn(actual.spawnSync) }
})

describe("bundled MicroSandbox release staging", () => {
  it("pins the runtime patch and shares retention rules with stopped sandboxes", () => {
    const inputs = JSON.parse(readFileSync("runtime-inputs.json", "utf8"))
    for (const patch of inputs.patches) {
      expect(sha256(readFileSync(patch.path))).toBe(patch.sha256)
    }
    const retentionPatch = MICROSANDBOX_PATCHES.find(patch => patch.path.includes("microsandbox-log-retention-desktop-start"))
    expect(retentionPatch).toBeDefined()
    const patch = readFileSync(retentionPatch!.path, "utf8")
    const marker = "+++ b/crates/runtime/lib/runner/logging_retention.rs\n"
    const section = patch.split(marker)[1]?.split("diff --git ")[0]
    expect(section).toBeDefined()
    const source = section!.split("\n").filter((line) => line.startsWith("+")).map((line) => line.slice(1)).join("\n") + "\n"
    expect(source).toBe(readFileSync("src-tauri/engine/src/log_retention.rs", "utf8"))
  })

  it("applies a source patch inside another repository without changing the parent", async () => {
    const parent = await mkdtemp(join(tmpdir(), "silo-nested-patch-"))
    try {
      execFileSync("git", ["init", "--quiet", parent])
      const source = join(parent, "build", "source")
      await mkdir(source, { recursive: true })
      await writeFile(join(parent, "value.txt"), "parent\n")
      await writeFile(join(source, "value.txt"), "before\n")
      const patch = join(parent, "change.patch")
      await writeFile(patch, "diff --git a/value.txt b/value.txt\n--- a/value.txt\n+++ b/value.txt\n@@ -1 +1 @@\n-before\n+after\n")
      applyRuntimePatch(source, patch)
      expect(await readFile(join(source, "value.txt"), "utf8")).toBe("after\n")
      expect(await readFile(join(parent, "value.txt"), "utf8")).toBe("parent\n")
    } finally {
      await rm(parent, { recursive: true, force: true })
    }
  })
  it("uses Tauri's requested target for cross-builds unless explicitly overridden", () => {
    const hostTriple = vi.fn(() => "aarch64-apple-darwin\n")

    expect(resolveRuntimeTarget({ TAURI_ENV_TARGET_TRIPLE: "x86_64-unknown-linux-gnu" }, hostTriple))
      .toBe("x86_64-unknown-linux-gnu")
    expect(resolveRuntimeTarget({
      SILO_RUNTIME_TARGET: "aarch64-unknown-linux-gnu",
      TAURI_ENV_TARGET_TRIPLE: "x86_64-unknown-linux-gnu",
    }, hostTriple)).toBe("aarch64-unknown-linux-gnu")
    expect(hostTriple).not.toHaveBeenCalled()
  })

  it.each([
    ["aarch64-apple-darwin", "msb-darwin-aarch64", "libkrunfw.5.dylib"],
    ["aarch64-unknown-linux-gnu", "msb-linux-aarch64", "libkrunfw.so.5.6.1"],
    ["x86_64-unknown-linux-gnu", "msb-linux-x86_64", "libkrunfw.so.5.6.1"],
  ])("selects the published matching pair for %s", (target, executable, library) => {
    expect(selectRuntime(target)).toMatchObject({ executableAsset: executable, libraryName: library })
  })

  it.each(["x86_64-apple-darwin", "aarch64-unknown-linux-musl", "riscv64-unknown-linux-gnu"])(
    "rejects an unsupported target instead of guessing for %s",
    (target) => expect(() => selectRuntime(target)).toThrow(`Unsupported bundled MicroSandbox target: ${target}`),
  )

  it("rejects corrupt bytes with both expected and actual digests", () => {
    const bytes = Buffer.from("corrupt")
    expect(() => verifySha256(bytes, "0".repeat(64), "msb-darwin-aarch64"))
      .toThrow(`expected ${"0".repeat(64)}, received ${sha256(bytes)}`)
  })

  it("does not publish a runtime manifest when a downloaded release asset is corrupt", async () => {
    const appRoot = await mkdtemp(join(tmpdir(), "silo-runtime-corrupt-test-"))
    const targetTriple = "aarch64-apple-darwin"
    const selected = { ...runtimeTargets[targetTriple], executableSha256: "0".repeat(64) }

    await expect(stageRuntime({
      appRoot,
      targetTriple,
      selected,
      licenses: [],
      fetchStream: async () => Readable.from([Buffer.from("corrupt-msb")]),
    })).rejects.toThrow("msb-darwin-aarch64 checksum mismatch")
    await expect(stat(join(appRoot, "src-tauri/runtime/microsandbox/manifest.json"))).rejects.toThrow()
  })

  it("stages verified assets at Tauri's target-qualified sidecar and resource paths", async () => {
    const appRoot = await mkdtemp(join(tmpdir(), "silo-runtime-test-"))
    const targetTriple = "aarch64-apple-darwin"
    const executable = Buffer.from("fake-msb")
    const agentd = Buffer.from("fake-agentd")
    const library = Buffer.from("fake-libkrunfw")
    const apache = Buffer.from("fake-apache")
    const selected = {
      ...runtimeTargets[targetTriple],
      executableSha256: sha256(executable),
      agentdSha256: sha256(agentd),
      librarySha256: sha256(library),
    }
    const licenses = [{ name: "LICENSE.txt", url: "https://example.test/LICENSE", sha256: sha256(apache) }]
    const patches = await Promise.all(MICROSANDBOX_PATCHES.map(async patch => Buffer.from(await readFile(join(process.cwd(), patch.path)))))
    const source = Buffer.from("fake pinned source")
    const sourceArtifact = { url: "https://example.test/source.tar.gz", sha256: sha256(source) }
    await mkdir(join(appRoot, "patches"), { recursive: true })
    for (const [index, patchInput] of MICROSANDBOX_PATCHES.entries()) {
      await mkdir(join(appRoot, "patches"), { recursive: true })
      await writeFile(join(appRoot, patchInput.path), patches[index])
    }

    const fetchStream = vi.fn(async (url: string) => {
      if (url.endsWith(`/${selected.executableAsset}`)) return Readable.from([executable])
      if (url.endsWith(`/${selected.agentdAsset}`)) return Readable.from([agentd])
      if (url === sourceArtifact.url) return Readable.from([source])
      if (url.endsWith(`/${selected.libraryAsset}`)) return Readable.from([library])
      if (url === licenses[0].url) return Readable.from([apache])
      throw new Error(`unexpected URL: ${url}`)
    })
    fetchStream.mockImplementation(async (url: string) => {
      if (url.endsWith(`/${selected.executableAsset}`)) return Readable.from([executable])
      if (url.endsWith(`/${selected.agentdAsset}`)) return Readable.from([agentd])
      if (url === sourceArtifact.url) return Readable.from([source])
      if (url.endsWith(`/${selected.libraryAsset}`)) return Readable.from([library])
      if (url === licenses[0].url) return Readable.from([apache])
      throw new Error(`unexpected URL: ${url}`)
    })
    const buildExecutable = vi.fn(async ({ sourceArchive }: { sourceArchive: string }) => {
      expect(await readFile(sourceArchive)).toEqual(source)
      return executable
    })
    const staged = await stageRuntime({ appRoot, targetTriple, fetchStream, selected, licenses, sourceArtifact, buildExecutable, verifyExecutable: false })

    expect(staged.executablePath).toBe(join(appRoot, "src-tauri/binaries/msb-aarch64-apple-darwin"))
    expect(staged.libraryPath).toBe(join(appRoot, "src-tauri/runtime/microsandbox/aarch64-apple-darwin/lib/libkrunfw.5.dylib"))
    expect(await readFile(staged.executablePath)).toEqual(executable)
    expect(await readFile(staged.libraryPath)).toEqual(library)
    expect((await stat(staged.executablePath)).mode & 0o777).toBe(0o755)
    const manifest = JSON.parse(await readFile(staged.manifestPath, "utf8"))
    expect(manifest).toMatchObject({
      microsandboxVersion: MICRO_SANDBOX_VERSION,
      targetTriple,
      executable: {
        bundledName: "msb",
        sha256: sha256(executable),
        sourceCommit: JSON.parse(readFileSync("runtime-inputs.json", "utf8")).sourceCommit,
        patchSha256s: MICROSANDBOX_PATCHES.map((_, index) => sha256(patches[index])),
        officialReleaseAsset: "msb-darwin-aarch64",
      },
      library: { bundledName: "libkrunfw.5.dylib", sha256: sha256(library) },
    })
    expect(manifest.library).not.toHaveProperty("resourcePath")
    expect(buildExecutable).toHaveBeenCalledOnce()
    expect(fetchStream).toHaveBeenCalledTimes(5)

    const originalManifest = await readFile(staged.manifestPath)
    await expect(stageRuntime({
      appRoot, targetTriple, fetchStream, selected, sourceArtifact, verifyExecutable: false,
      buildExecutable: async () => Buffer.from("replacement compiled executable"),
      licenses: [{ ...licenses[0], name: "new-license.txt", url: "https://example.test/missing-license" }],
    })).rejects.toThrow("unexpected URL")
    expect(await readFile(staged.executablePath)).toEqual(executable)
    expect(await readFile(staged.manifestPath)).toEqual(originalManifest)
    expect(await readFile(staged.libraryPath)).toEqual(library)
  })

  async function compiledRuntimeFixture() {
    const appRoot = await mkdtemp(join(tmpdir(), "silo-agentd-cache-test-"))
    const targetTriple = "aarch64-apple-darwin"
    const releaseExecutable = Buffer.from("official release fixture")
    const source = Buffer.from("unchanged source archive fixture")
    const library = Buffer.from("library fixture")
    let agentd = Buffer.from("agent revision one")
    const state = { compilations: 0 }
    const outdated = {
      outdatedCachedSsh: false,
      outdatedCachedAccount: false,
      outdatedCachedStdin: false,
      outdatedCachedInstance: false,
      outdatedCachedStaging: false,
      outdatedCachedNoStdin: false,
      outdatedCachedRestoreStrict: false,
      outdatedCachedRestoreHelp: false,
    }
    const sourceArtifact = { url: "https://example.test/source.tar.gz", sha256: sha256(source) }
    const selected = {
      ...runtimeTargets[targetTriple],
      executableSha256: sha256(releaseExecutable),
      agentdSha256: sha256(agentd),
      librarySha256: sha256(library),
    }
    await mkdir(join(appRoot, "patches"), { recursive: true })
    await Promise.all(MICROSANDBOX_PATCHES.map(async patch => writeFile(join(appRoot, patch.path), await readFile(join(process.cwd(), patch.path)))))
    // The compiled runtime also bundles the macOS gateway, which the build reads from the app root.
    await cp(join(process.cwd(), "src-tauri/macos-gateway"), join(appRoot, "src-tauri/macos-gateway"), { recursive: true })
    const fetchStream = async (url: string) => {
      if (url === sourceArtifact.url) return Readable.from([source])
      if (url.endsWith(`/${selected.executableAsset}`)) return Readable.from([releaseExecutable])
      if (url.endsWith(`/${selected.agentdAsset}`)) return Readable.from([agentd])
      if (url.endsWith(`/${selected.libraryAsset}`)) return Readable.from([library])
      throw new Error(`Unexpected download: ${url}`)
    }
    // Exercise the real download verifier, build-cache lookup, compiler orchestration,
    // and staged bytes. Only external tools are replaced with deterministic fixtures.
    const compiler = ((command: string, args: string[], options: ExecFileSyncOptions) => {
      if (command === "rustc") return `rustc ${MICROSANDBOX_BUILD_TOOLCHAIN} (fixture)`
      if (command === "/usr/bin/tar") {
        mkdirSync(join(args[args.indexOf("-C") + 1], "source"), { recursive: true })
        return ""
      }
      if (command === "/usr/bin/git") return ""
      if (command === "cargo" && args.includes("fetch")) return ""
      if (command === "cargo") {
        const output = join(String(options.env?.CARGO_TARGET_DIR), targetTriple, "release")
        mkdirSync(output, { recursive: true })
        if (args.includes("silo-macos-gateway")) {
          // The gateway is built separately from the runtime and does not embed the agent.
          writeFileSync(join(output, "silo-macos-gateway"), Buffer.from("compiled gateway"))
          return ""
        }
        state.compilations += 1
        const embeddedAgent = readFileSync(join(String(options.cwd), "build/agentd"))
        writeFileSync(join(output, "msb"), Buffer.concat([Buffer.from("compiled runtime:"), embeddedAgent]))
        return ""
      }
      if (command.startsWith(appRoot) && command.endsWith("/msb")) {
        // A build's own output is in its cargo target; a cached copy is probed from a work folder.
        const fresh = command.includes("/release/msb")
        if (outdated.outdatedCachedAccount && !fresh && args[0] === "--silo-working-account-protocol") return "0"
        if (outdated.outdatedCachedStdin && !fresh && args[0] === "--silo-secret-values-protocol") return "0"
        if (outdated.outdatedCachedInstance && !fresh && args[0] === "--silo-runtime-instance-protocol") return "0"
        if (outdated.outdatedCachedRestoreStrict && !fresh && args[0] === "--silo-restore-strict-protocol") return "0"
        if (["--silo-storage-protocol", "--silo-desktop-protocol", "--silo-github-token-protocol", "--silo-github-protocol", "--silo-working-account-protocol", "--silo-secret-values-protocol", "--silo-runtime-instance-protocol", "--silo-restore-strict-protocol"].includes(args[0])) return "1"
        if (args[0] === "--version") return `msb ${MICRO_SANDBOX_VERSION}`
        if (outdated.outdatedCachedStaging && !fresh && args[0] === "snapshot" && args[1] === "load") return "--group"
        if (args.includes("--help")) {
          const oldFlags = "--mount-owned --no-start --progress-json"
          const currentFlags = `${oldFlags} --no-stdin`
          if (args[0] === "snapshot") return "--from-sandbox --group --dest-dir --full --guest-flush --integrity --stage-id"
          if (args[0] === "restore") {
            return outdated.outdatedCachedRestoreHelp && !fresh ? "--cow-mem --name" : "--cow-mem --name --net-strict"
          }
          if (outdated.outdatedCachedNoStdin && !fresh && args[0] === "exec") return oldFlags
          return outdated.outdatedCachedSsh && !fresh && args[0] === "ssh"
            ? oldFlags
            : `${currentFlags} --authorized-keys --exit-on-stdin-close --expected-machine-id`
        }
      }
      throw new Error(`Unexpected tool invocation: ${command} ${args.join(" ")}`)
    }) as typeof execFileSync
    const stage = () => stageRuntime({ appRoot, targetTriple, fetchStream, selected, licenses: [], sourceArtifact, verifyExecutable: false })
    // The build tool identity asks the developer tools for their versions; fixed answers keep
    // the fixture fast and the same on every machine.
    const realSpawnSync = vi.mocked(spawnSync).getMockImplementation()!
    const identityTools = new Set(["xcode-select", "xcodebuild", "xcrun", "cc", "c++", "clang", "ld"])
    const tools = ((command: string, args: string[], options: SpawnSyncOptions) => identityTools.has(command)
      ? { pid: 0, output: [], stdout: `${command} fixture`, stderr: "", status: 0, signal: null }
      : realSpawnSync(command, args, options)) as typeof spawnSync
    const run = (body: () => Promise<void>) => vi.mocked(spawnSync).withImplementation(tools, () => vi.mocked(execFileSync).withImplementation(compiler, body))
    const changeAgent = (revision: string) => {
      agentd = Buffer.from(revision)
      selected.agentdSha256 = sha256(agentd)
    }
    return { appRoot, state, outdated, stage, run, changeAgent, getAgent: () => agentd }
  }

  async function withCompiledRuntime(body: (fixture: Awaited<ReturnType<typeof compiledRuntimeFixture>>) => Promise<void>) {
    const fixture = await compiledRuntimeFixture()
    try {
      await fixture.run(() => body(fixture))
    } finally {
      vi.unstubAllEnvs()
      await rm(fixture.appRoot, { recursive: true, force: true })
    }
  }

  it("reuses an unchanged compiled runtime but rebuilds when verified embedded agent bytes change", () => withCompiledRuntime(async ({ stage, state, changeAgent, getAgent }) => {
    const first = await stage()
    const firstBytes = await readFile(first.executablePath)
    expect(firstBytes.toString()).toBe("compiled runtime:agent revision one")
    expect(state.compilations).toBe(1)

    const warm = await stage()
    expect(await readFile(warm.executablePath)).toEqual(firstBytes)
    expect(state.compilations).toBe(1)

    changeAgent("agent revision two")
    const changed = await stage()
    expect(await readFile(changed.executablePath, "utf8")).toBe("compiled runtime:agent revision two")
    expect(state.compilations).toBe(2)
    const manifest = JSON.parse(await readFile(changed.manifestPath, "utf8"))
    expect(manifest.executable.embeddedAgentdReleaseSha256).toBe(sha256(getAgent()))
    expect(manifest.executable.sha256).toBe(sha256(await readFile(changed.executablePath)))
  }))

  // A cached runtime that fails one capability probe or help check is rebuilt.
  it.each([
    "outdatedCachedSsh",
    "outdatedCachedAccount",
    "outdatedCachedStdin",
    "outdatedCachedInstance",
    "outdatedCachedStaging",
    "outdatedCachedNoStdin",
    "outdatedCachedRestoreStrict",
    "outdatedCachedRestoreHelp",
  ] as const)("rebuilds a cached runtime that is %s", (flag) => withCompiledRuntime(async ({ stage, state, outdated }) => {
    await stage()
    expect(state.compilations).toBe(1)
    outdated[flag] = true
    await stage()
    expect(state.compilations).toBe(2)
    outdated[flag] = false
    await stage()
    expect(state.compilations).toBe(2)
  }))

  it.each([
    ["RUSTFLAGS", "-C opt-level=1"],
    ["CARGO_ENCODED_RUSTFLAGS", "-C\u001fopt-level=2"],
    ["CARGO_BUILD_RUSTFLAGS", "-C opt-level=3"],
    ["CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS", "-C target-cpu=generic"],
    ["CARGO_PROFILE_RELEASE_DEBUG", "2"],
    ["CARGO_PROFILE_RELEASE_OPT_LEVEL", "1"],
    ["CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_OPT_LEVEL", "1"],
  ])("rebuilds a cached runtime once when %s changes", (name, value) => withCompiledRuntime(async ({ stage, state }) => {
    await stage()
    expect(state.compilations).toBe(1)
    vi.stubEnv(name, value)
    await stage()
    expect(state.compilations).toBe(2)
    await stage()
    expect(state.compilations).toBe(2)
  }))

  it("does not rebuild when only the order of equivalent profile variables changes", () => withCompiledRuntime(async ({ stage, state }) => {
    vi.stubEnv("CARGO_PROFILE_RELEASE_DEBUG", "2")
    vi.stubEnv("CARGO_PROFILE_RELEASE_OPT_LEVEL", "1")
    await stage()
    expect(state.compilations).toBe(1)
    delete process.env.CARGO_PROFILE_RELEASE_DEBUG
    vi.stubEnv("CARGO_PROFILE_RELEASE_DEBUG", "2")
    await stage()
    expect(state.compilations).toBe(1)
  }))

  it("rebuilds the gateway when its published-port code changes", () => withCompiledRuntime(async ({ appRoot, stage, state }) => {
    await stage()
    expect(state.compilations).toBe(1)
    const ports = join(appRoot, "src-tauri/macos-gateway/src/ports.rs")
    await writeFile(ports, `${await readFile(ports, "utf8")}\n// changed\n`)
    await stage()
    expect(state.compilations).toBe(2)
  }))

})
