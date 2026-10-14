import { Readable } from "node:stream"
import { execFileSync, type ExecFileSyncOptions } from "node:child_process"
import { mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"

import { describe, expect, it, vi } from "vitest"
import {
  applyRuntimePatch,
  runtimeTargets,
  MICROSANDBOX_PATCHES,
  MICRO_SANDBOX_VERSION,
  resolveRuntimeTarget,
  selectRuntime,
  sha256,
  stageRuntime,
  verifySha256,
} from "../../scripts/microsandbox-runtime.mjs"

vi.mock("node:child_process", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:child_process")>()
  return { ...actual, execFileSync: vi.fn(actual.execFileSync) }
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
      microsandboxVersion: "0.7.6",
      targetTriple,
      executable: {
        bundledName: "msb",
        sha256: sha256(executable),
        sourceCommit: "09df3d4b9d832adaede1fb9a198cfc660bfab8cd",
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

  it("reuses an unchanged compiled runtime but rebuilds when verified embedded agent bytes change", async () => {
    const appRoot = await mkdtemp(join(tmpdir(), "silo-agentd-cache-test-"))
    const targetTriple = "aarch64-apple-darwin"
    const releaseExecutable = Buffer.from("official release fixture")
    const source = Buffer.from("unchanged source archive fixture")
    const library = Buffer.from("library fixture")
    let agentd = Buffer.from("agent revision one")
    let compilations = 0
    let outdatedCachedSsh = false
    let outdatedCachedAccount = false
    let outdatedCachedStdin = false
    let outdatedCachedInstance = false
    let outdatedCachedStaging = false
    let outdatedCachedNoStdin = false
    const sourceArtifact = { url: "https://example.test/source.tar.gz", sha256: sha256(source) }
    const selected = {
      ...runtimeTargets[targetTriple],
      executableSha256: sha256(releaseExecutable),
      agentdSha256: sha256(agentd),
      librarySha256: sha256(library),
    }
    for (const patch of MICROSANDBOX_PATCHES) {
      await mkdir(join(appRoot, "patches"), { recursive: true })
      await writeFile(join(appRoot, patch.path), await readFile(join(process.cwd(), patch.path)))
    }
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
      if (command === "rustc") return "rustc 1.94.0 (fixture)"
      if (command === "/usr/bin/tar") {
        mkdirSync(join(args[args.indexOf("-C") + 1], "source"), { recursive: true })
        return ""
      }
      if (command === "/usr/bin/git") return ""
      if (command === "cargo" && args.includes("fetch")) return ""
      if (command === "cargo") {
        compilations += 1
        const output = join(String(options.env?.CARGO_TARGET_DIR), targetTriple, "release")
        mkdirSync(output, { recursive: true })
        const embeddedAgent = readFileSync(join(String(options.cwd), "build/agentd"))
        writeFileSync(join(output, "msb"), Buffer.concat([Buffer.from("compiled runtime:"), embeddedAgent]))
        return ""
      }
      if (command.startsWith(appRoot) && command.endsWith("/msb")) {
        if (outdatedCachedAccount && !command.includes("cargo-target") && args[0] === "--silo-working-account-protocol") return "0"
        if (outdatedCachedStdin && !command.includes("cargo-target") && args[0] === "--silo-secret-values-protocol") return "0"
        if (outdatedCachedInstance && !command.includes("cargo-target") && args[0] === "--silo-runtime-instance-protocol") return "0"
        if (["--silo-storage-protocol", "--silo-desktop-protocol", "--silo-github-token-protocol", "--silo-github-protocol", "--silo-working-account-protocol", "--silo-secret-values-protocol", "--silo-runtime-instance-protocol"].includes(args[0])) return "1"
        if (args[0] === "--version") return `msb ${MICRO_SANDBOX_VERSION}`
        if (outdatedCachedStaging && !command.includes("cargo-target") && args[0] === "snapshot" && args[1] === "load") return "--group"
        if (args.includes("--help")) {
          const oldFlags = "--mount-owned --no-start --progress-json"
          const currentFlags = `${oldFlags} --no-stdin`
          if (args[0] === "snapshot") return "--from-sandbox --group --dest-dir --full --guest-flush --integrity --stage-id"
          if (args[0] === "restore") return "--cow-mem --name"
          if (outdatedCachedNoStdin && !command.includes("cargo-target") && args[0] === "exec") return oldFlags
          return outdatedCachedSsh && !command.includes("cargo-target") && args[0] === "ssh"
            ? oldFlags
            : `${currentFlags} --authorized-keys --exit-on-stdin-close --expected-machine-id`
        }
      }
      throw new Error(`Unexpected tool invocation: ${command} ${args.join(" ")}`)
    }) as typeof execFileSync
    try {
      await vi.mocked(execFileSync).withImplementation(compiler, async () => {
        const stage = () => stageRuntime({ appRoot, targetTriple, fetchStream, selected, licenses: [], sourceArtifact, verifyExecutable: false })
        const first = await stage()
        const firstBytes = await readFile(first.executablePath)
        expect(firstBytes.toString()).toBe("compiled runtime:agent revision one")
        expect(compilations).toBe(1)

        const warm = await stage()
        expect(await readFile(warm.executablePath)).toEqual(firstBytes)
        expect(compilations).toBe(1)

        outdatedCachedSsh = true
        await stage()
        expect(compilations).toBe(2)
        outdatedCachedSsh = false

        outdatedCachedAccount = true
        await stage()
        expect(compilations).toBe(3)
        outdatedCachedAccount = false

        outdatedCachedStdin = true
        await stage()
        expect(compilations).toBe(4)
        outdatedCachedStdin = false

        // A cached runtime whose inspect output lacks the running instance is rebuilt.
        outdatedCachedInstance = true
        await stage()
        expect(compilations).toBe(5)
        outdatedCachedInstance = false

        outdatedCachedStaging = true
        await stage()
        expect(compilations).toBe(6)
        outdatedCachedStaging = false

        outdatedCachedNoStdin = true
        await stage()
        expect(compilations).toBe(7)
        outdatedCachedNoStdin = false

        agentd = Buffer.from("agent revision two")
        selected.agentdSha256 = sha256(agentd)
        const changed = await stage()
        expect(await readFile(changed.executablePath, "utf8")).toBe("compiled runtime:agent revision two")
        expect(compilations).toBe(8)
        const manifest = JSON.parse(await readFile(changed.manifestPath, "utf8"))
        expect(manifest.executable.embeddedAgentdReleaseSha256).toBe(sha256(agentd))
        expect(manifest.executable.sha256).toBe(sha256(await readFile(changed.executablePath)))

        vi.stubEnv("RUSTFLAGS", "-C opt-level=1")
        await stage()
        expect(compilations).toBe(9)
        await stage()
        expect(compilations).toBe(9)

        vi.stubEnv("CARGO_ENCODED_RUSTFLAGS", "-C\u001fopt-level=2")
        await stage()
        expect(compilations).toBe(10)

        vi.stubEnv("CARGO_BUILD_RUSTFLAGS", "-C opt-level=3")
        await stage()
        expect(compilations).toBe(11)
        vi.stubEnv("CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS", "-C target-cpu=generic")
        await stage()
        expect(compilations).toBe(12)
        await stage()
        expect(compilations).toBe(12)

        vi.stubEnv("CARGO_PROFILE_RELEASE_DEBUG", "2")
        await stage()
        expect(compilations).toBe(13)
        await stage()
        expect(compilations).toBe(13)
        vi.stubEnv("CARGO_PROFILE_RELEASE_OPT_LEVEL", "1")
        await stage()
        expect(compilations).toBe(14)
        vi.stubEnv("CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_OPT_LEVEL", "1")
        await stage()
        expect(compilations).toBe(15)
        // Environment insertion order does not change an equivalent profile.
        delete process.env.CARGO_PROFILE_RELEASE_DEBUG
        vi.stubEnv("CARGO_PROFILE_RELEASE_DEBUG", "2")
        await stage()
        expect(compilations).toBe(15)
      })
    } finally {
      vi.unstubAllEnvs()
      await rm(appRoot, { recursive: true, force: true })
    }
  })

})
