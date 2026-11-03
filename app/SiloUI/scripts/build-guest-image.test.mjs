import assert from "node:assert/strict"
import { execFileSync, spawnSync } from "node:child_process"
import { existsSync, readFileSync } from "node:fs"
import { mkdir, mkdtemp, readFile, readdir, realpath, rm, symlink, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { gunzipSync } from "node:zlib"
import { createHash } from "node:crypto"
import test from "node:test"
import { GUEST_IMAGE_VERSION, guestImageMetadata, cuaDriverArchive, cuaDriverShim, verifyGuestImage } from "./build-guest-image.mjs"

test("metadata CLI works through a symlink to the script", async t => {
  const root = await mkdtemp(join(tmpdir(), "silo-guest-cli-link-"))
  t.after(() => rm(root, { recursive: true, force: true }))
  const entry = join(root, "guest-image.mjs")
  await symlink(new URL("./build-guest-image.mjs", import.meta.url), entry)
  const result = spawnSync(process.execPath, [entry, "metadata"], { encoding: "utf8",
    env: { ...process.env, GITHUB_REPOSITORY: "fixture/silo" } })
  assert.equal(result.status, 0, result.stderr)
  assert.match(result.stdout, new RegExp(`^version=${GUEST_IMAGE_VERSION}$`, "m"))
  assert.match(result.stdout, /^image=ghcr.io\/fixture\/silo-guest:/m)
})

for (const saveExit of [1, 0, "write-error"]) {
  test(`guest archive publication preserves complete outputs for ${saveExit === "write-error" ? "an archive write failure" : `docker save exit ${saveExit}`}`, async t => {
    const root = await realpath(await mkdtemp(join(tmpdir(), "silo-guest-save-")))
    t.after(() => rm(root, { recursive: true, force: true }))
    const scripts = join(root, "scripts")
    const guest = join(root, "src-tauri/guest")
    const output = join(root, "src-tauri/guest-image-artifacts/arm64")
    const commands = join(root, "commands")
    for (const directory of [scripts, guest, output, commands]) await mkdir(directory, { recursive: true })
    const script = join(scripts, "build-guest-image.mjs")
    await writeFile(script, await readFile(new URL("./build-guest-image.mjs", import.meta.url)))
    await writeFile(join(guest, "cua-driver-lock.json"), await readFile(new URL("../src-tauri/guest/cua-driver-lock.json", import.meta.url)))
    await writeFile(join(guest, "verify-tools.sh"), "true\n")
    await writeFile(join(guest, "cua-driver-shim"), await readFile(new URL("../src-tauri/guest/cua-driver-shim", import.meta.url)))
    await writeFile(join(root, "package.json"), '{"repository":{"url":"https://github.com/fixture/silo"}}')
    await writeFile(join(output, "image.tar.gz"), "previous verified archive")
    await writeFile(join(output, "manifest.json"), "previous manifest")
    const writeError = saveExit === "write-error"
    const preload = join(root, "fail-archive-write.mjs")
    if (writeError) {
      await writeFile(preload, `
import fs from 'node:fs'
import { syncBuiltinESMExports } from 'node:module'
import { Writable } from 'node:stream'
fs.createWriteStream = () => new Writable({ write(chunk, encoding, callback) { callback(new Error('fixture ENOSPC')) } })
syncBuiltinESMExports()
`)
    }
    const docker = `#!${process.execPath}
const args = process.argv.slice(2)
if (args[0] === 'image' && args[1] === 'inspect') console.log(JSON.stringify([{ Id: 'sha256:fixture' }]))
if (args[0] === 'run' && args.at(-1) === '/usr/local/share/silo-packages.txt') console.log('fixture-package\\t1')
if (args[0] === 'image' && args[1] === 'save') {
  ${writeError ? `require('node:fs').writeFileSync(${JSON.stringify(join(root, "save.pid"))}, String(process.pid))
  process.stdout.on('error', () => {})
  process.stdout.write('saved image fixture')
  setTimeout(() => process.exit(0), 15000)` : `process.stdout.write('saved image fixture', () => process.exit(${saveExit}))`}
}
`
    await writeFile(join(commands, "docker"), docker, { mode: 0o755 })
    const result = spawnSync(process.execPath, [...(writeError ? ["--import", preload] : []), script, "arm64"], { encoding: "utf8", timeout: 8000, env: {
      ...process.env, PATH: `${commands}:${dirname(process.execPath)}`, GITHUB_REPOSITORY: "fixture/silo", GITHUB_SHA: "fixture",
    } })
    if (writeError && result.error?.code === "ETIMEDOUT") {
      try {
        const pid = Number(await readFile(join(root, "save.pid"), "utf8"))
        process.kill(pid, "SIGTERM")
      } catch (error) {
        if (!["ENOENT", "ESRCH"].includes(error.code)) throw error
      }
    }
    assert.equal(result.status, writeError ? 1 : saveExit, result.stderr)
    if (writeError) {
      assert.match(result.stderr, /fixture ENOSPC/)
      const pid = Number(await readFile(join(root, "save.pid"), "utf8"))
      assert.throws(() => process.kill(pid, 0), { code: "ESRCH" })
    }
    if (saveExit) {
      assert.equal(await readFile(join(output, "image.tar.gz"), "utf8"), "previous verified archive")
      assert.equal(await readFile(join(output, "manifest.json"), "utf8"), "previous manifest")
    } else {
      const archive = await readFile(join(output, "image.tar.gz"))
      assert.equal(gunzipSync(archive).toString(), "saved image fixture")
      const manifest = JSON.parse(await readFile(join(output, "manifest.json"), "utf8"))
      assert.equal(manifest.archiveSha256, createHash("sha256").update(archive).digest("hex"))
      assert.equal(manifest.archiveBytes, archive.length)
      assert.equal(manifest.unpackedBytes, Buffer.byteLength("saved image fixture"))
      assert.equal(manifest.architecture, "aarch64")
      assert.deepEqual(manifest.packages, { "fixture-package": "1" })
    }
    assert.deepEqual((await readdir(output)).sort(), ["image.tar.gz", "manifest.json"])
  })
}

test("publication names derive from the recipe version and the publishing repository", () => {
  assert.deepEqual(guestImageMetadata({ GITHUB_REPOSITORY: "Example-Owner/silo" }), {
    version: GUEST_IMAGE_VERSION,
    image: `ghcr.io/example-owner/silo-guest:${GUEST_IMAGE_VERSION}`,
    tag: `guest-${GUEST_IMAGE_VERSION}`,
    title: `Silo guest ${GUEST_IMAGE_VERSION.replace(/^ubuntu-([\d.]+)-(v\d+)$/, "Ubuntu $1 $2")}`,
  })
  // Local builds fall back to the package's repository.
  assert.match(guestImageMetadata({}).image, /^ghcr\.io\/[a-z0-9-]+\/silo-guest:/)
})

test("the publication workflow repeats neither the image version nor the owner", () => {
  const workflow = readFileSync(new URL("../../../.github/workflows/guest-image.yml", import.meta.url), "utf8")
  assert.equal(workflow.includes(GUEST_IMAGE_VERSION), false)
  assert.doesNotMatch(workflow, /ghcr\.io\/[a-z0-9]/)
  assert.match(workflow, /environment: guest-image-publish/)
})

test("the recipe matches the version and never leaves package files in a layer", () => {
  const dockerfile = readFileSync(new URL("../guest-image/Dockerfile", import.meta.url), "utf8")
  assert.ok(dockerfile.includes(`org.opencontainers.image.version="${GUEST_IMAGE_VERSION}"`))
  // Package files are downloaded, verified and deleted in one RUN, or bind-mounted; never COPYed.
  assert.doesNotMatch(dockerfile, /^COPY .*\.deb/m)
  assert.match(dockerfile, /RUN --mount=type=bind,source=src-tauri\/guest\/desktop-streamer-lock\.json/)
  assert.match(dockerfile, /sha256sum --check/)
  assert.doesNotMatch(dockerfile.replace(/^#.*$/gm, ""), /mousepad/)
  assert.match(dockerfile, /gnome-text-editor/)
  // The runtime installer detects v4 by this marker; it must match the recipe version.
  assert.ok(dockerfile.includes(`"version":"${GUEST_IMAGE_VERSION}"`))
  assert.match(dockerfile, /"capabilities":\["desktop","accessibility","cua-driver"\]/)
  assert.ok(dockerfile.includes("/usr/local/share/silo/guest-image.json"))
  assert.doesNotMatch(dockerfile.replace(/^#.*$/gm, ""), /openai|oaistatic|chatgpt/i)
  const ignore = readFileSync(new URL("../guest-image/Dockerfile.dockerignore", import.meta.url), "utf8")
  for (const input of ["desktop-streamer-lock.json", "desktop-packages.txt", "silo-accessibility.py", "silo-cua-driver-mcp", "cua-driver-shim", "setup-github.sh", "guest-third-party-notices.md"]) {
    assert.ok(ignore.includes(`!src-tauri/guest/${input}`), input)
  }
})

test("the image ships the third-party notices that THIRD-PARTY-NOTICES.md repeats", () => {
  const dockerfile = readFileSync(new URL("../guest-image/Dockerfile", import.meta.url), "utf8")
  assert.match(dockerfile, /source=src-tauri\/guest\/guest-third-party-notices\.md,target=\/mnt\/guest-third-party-notices\.md/)
  assert.ok(dockerfile.includes("/mnt/guest-third-party-notices.md /usr/share/doc/silo-guest-third-party/NOTICES.md"))
  const notices = readFileSync(new URL("../src-tauri/guest/guest-third-party-notices.md", import.meta.url), "utf8")
  const bundled = readFileSync(new URL("../THIRD-PARTY-NOTICES.md", import.meta.url), "utf8")
  assert.ok(bundled.includes(notices.trim()), "THIRD-PARTY-NOTICES.md must contain the guest notices verbatim")
  for (const required of ["x264", "x265", "n8.1", "guest-ubuntu-24.04-v4-source", "written offer", "Cua Driver", "Permission is hereby granted"]) {
    assert.ok(notices.toLowerCase().includes(required.toLowerCase()), required)
  }
})

test("the release attaches every guest input the image build consumes", () => {
  const dockerfile = readFileSync(new URL("../guest-image/Dockerfile", import.meta.url), "utf8")
  const workflow = readFileSync(new URL("../../../.github/workflows/guest-image.yml", import.meta.url), "utf8")
  const inputs = new Set([...dockerfile.replace(/^#.*$/gm, "").matchAll(/source=src-tauri\/guest\/([A-Za-z0-9._-]+)/g)].map((match) => match[1]))
  assert.ok(inputs.has("cua-driver-lock.json"))
  for (const input of inputs) {
    assert.ok(workflow.includes(`cp app/SiloUI/src-tauri/guest/${input} guest-release/${input}`), input)
  }
})

test("the pinned Cua Driver is installed from one lock, hash-checked, binary only, telemetry off", () => {
  const dockerfile = readFileSync(new URL("../guest-image/Dockerfile", import.meta.url), "utf8")
  const code = dockerfile.replace(/^#.*$/gm, "")
  assert.match(code, /source=src-tauri\/guest\/cua-driver-lock\.json,target=\/mnt\/cua-driver-lock\.json/)
  assert.match(code, /echo "\$cua_sha256  \/tmp\/cua-driver\.tar\.gz" \| sha256sum --check --status/)
  assert.match(code, /stat -c %s \/tmp\/cua-driver\.tar\.gz/)
  assert.ok(code.includes('echo "$cua_binary_sha256  /usr/local/lib/silo/cua-driver/$cua_version/cua-driver" | sha256sum --check --status'))
  assert.match(code, /--proto-redir '=https'/)
  // Only the binary is extracted, to the versioned directory the /usr/local/bin shim runs.
  assert.match(code, /tar -xzf \/tmp\/cua-driver\.tar\.gz -C "\/usr\/local\/lib\/silo\/cua-driver\/\$cua_version" --no-same-owner cua-driver/)
  assert.match(code, /source=src-tauri\/guest\/cua-driver-shim,target=\/mnt\/cua-driver-shim/)
  assert.ok(code.includes('sed "s#@DRIVER@#/usr/local/lib/silo/cua-driver/$cua_version/cua-driver#" /mnt/cua-driver-shim > /usr/local/bin/cua-driver'))
  assert.doesNotMatch(code, /ln -sf .*\/usr\/local\/bin\/cua-driver/)
  // The shim switches telemetry and the update check off for every caller and runs the pinned binary.
  const pinnedVersion = cuaDriverArchive("arm64").version
  const shim = cuaDriverShim(pinnedVersion)
  assert.match(shim, /^#!\/bin\/sh\n/)
  assert.ok(shim.includes("export CUA_DRIVER_RS_TELEMETRY_ENABLED=0 CUA_DRIVER_RS_UPDATE_CHECK=false DO_NOT_TRACK=1\n"))
  assert.ok(shim.endsWith(`exec /usr/local/lib/silo/cua-driver/${pinnedVersion}/cua-driver "$@"\n`))
  assert.doesNotMatch(shim, /@DRIVER@/)
  assert.match(code, /CUA_DRIVER_RS_TELEMETRY_ENABLED=0/)
  assert.match(code, /CUA_DRIVER_RS_UPDATE_CHECK=false/)
  assert.match(code, /\/etc\/profile\.d\/silo-cua-driver\.sh/)
  assert.match(code, /"capabilities":\["desktop","accessibility","cua-driver"\],"streamerVersion":"2\.0\.0","cuaDriverVersion":/)
  assert.doesNotMatch(code, /lcu|bubblewrap|ffmpeg|mesa-vulkan/i)
  // The lock is read, never copied into a layer.
  assert.doesNotMatch(code, /COPY .*cua-driver/)
  const ignore = readFileSync(new URL("../guest-image/Dockerfile.dockerignore", import.meta.url), "utf8")
  assert.ok(ignore.includes("!src-tauri/guest/cua-driver-lock.json"))
  assert.doesNotMatch(ignore, /lcu/)
  // Every library the binary links is installed explicitly (ldd of Cua Driver 0.38.0).
  for (const library of ["libx11-6", "libxi6", "libxkbcommon0", "libxcb1", "libxext6", "libxau6", "libxdmcp6", "libbsd0", "libmd0", "libxtst6"]) {
    assert.ok(code.includes(library), library)
  }
  // One edit bumps Cua Driver: the lock names both architectures.
  const lock = JSON.parse(readFileSync(new URL("../src-tauri/guest/cua-driver-lock.json", import.meta.url), "utf8"))
  assert.deepEqual(Object.keys(lock).sort(), ["assets", "schemaVersion", "source", "version"])
  assert.deepEqual(Object.keys(lock.assets).sort(), ["amd64", "arm64"])
  for (const architecture of ["arm64", "amd64"]) {
    const archive = cuaDriverArchive(architecture)
    assert.equal(archive.version, lock.version)
    assert.match(archive.name, new RegExp(`^cua-driver-rs-${lock.version.replaceAll(".", "\\.")}-linux-(arm64|x86_64)-binary\\.tar\\.gz$`))
    assert.match(archive.sha256, /^[0-9a-f]{64}$/)
    // The installed binary is checked against its own digest, not only the archive's.
    assert.match(archive.binarySha256, /^[0-9a-f]{64}$/)
    assert.notEqual(archive.binarySha256, archive.sha256)
    assert.ok(archive.bytes > 0)
  }
  assert.throws(() => cuaDriverArchive("riscv64"), /Cua Driver lock/)
})

test("no Linux guest file refers to the removed LCU or ChatGPT downloads", () => {
  for (const removed of ["lcu-lock.json", "lcu-legacy-lock.json", "chatgpt-app-lock.json", "setup-lcu.py"]) {
    assert.equal(existsSync(new URL(`../src-tauri/guest/${removed}`, import.meta.url)), false, removed)
  }
})

test("offline image validation rejects unsupported platforms before invoking Docker", () => {
  assert.throws(() => verifyGuestImage("riscv64", "unused", {
    run: () => assert.fail("Unsupported image must not run"),
  }), /architecture/)
})

test("offline validation forbids image pulls and preserves tool-check failures", () => {
  const failedCheck = new Error("Guest tool check failed")
  assert.throws(() => verifyGuestImage("arm64", "local-fixture", {
    run(command, args) {
      assert.equal(command, "docker")
      assert.equal(args[0], "run")
      assert.equal(args[args.indexOf("--network") + 1], "none")
      assert.equal(args[args.indexOf("--pull") + 1], "never")
      assert.equal(args[args.indexOf("--platform") + 1], "linux/arm64")
      assert.ok(args.includes("--rm"))
      throw failedCheck
    },
  }), error => error === failedCheck)
})

// Runs only against an explicitly supplied, already-built image. Each failure
// fixture mutates its disposable container; neither the image nor host changes.
const image = process.env.SILO_TEST_GUEST_IMAGE
const architecture = process.env.SILO_TEST_GUEST_ARCHITECTURE || "arm64"
test("bundled guest tools and unprivileged SFTP work without network", { skip: !image }, () => {
  verifyGuestImage(architecture, image)
})
for (const [name, mutation] of [
  ["missing sudo", "rm -f /usr/bin/sudo"],
  ["missing Python", "rm -f /usr/bin/python3"],
  ["SFTP executable unavailable to normal users", "chmod 0700 /usr/lib/openssh/sftp-server"],
  ["preinstalled working account", "useradd --no-create-home silo"],
  ["missing dconf user profile", "rm -f /etc/dconf/profile/user"],
  ["invalid accessibility poller", "echo 'def (' > /usr/local/libexec/silo-accessibility"],
  ["autostart entry naming a missing program", "sed -i 's#^Exec=.*#Exec=/usr/local/libexec/missing#' /etc/xdg/autostart/silo-accessibility.desktop"],
  ["leftover package file", "touch /var/cache/selkies.deb"],
  ["a different Cua Driver binary", "echo x >> /usr/local/lib/silo/cua-driver/*/cua-driver"],
  ["a link in place of the telemetry-off shim", "ln -sf /usr/local/lib/silo/cua-driver/*/cua-driver /usr/local/bin/cua-driver"],
  ["a shim that keeps telemetry on", "sed -i 's/DO_NOT_TRACK=1//' /usr/local/bin/cua-driver"],
  ["an extra file beside Cua Driver", "touch /usr/local/lib/silo/cua-driver/*/libcua_driver_sdk.so"],
  ["telemetry on system-wide", "sed -i /TELEMETRY/d /etc/environment"],
  ["missing MCP launcher", "rm -f /usr/local/libexec/silo-cua-driver-mcp"],
  ["LCU installed in the image", "mkdir -p /opt/lcu"],
]) {
  test(`offline image check rejects ${name}`, { skip: !image }, () => {
    assert.throws(() => verifyGuestImage(architecture, image, {
      run(command, args) {
        const mutated = [...args]
        mutated[mutated.length - 1] = `${mutation}\n${mutated.at(-1)}`
        execFileSync(command, mutated, { stdio: "pipe" })
      },
    }))
  })
}
