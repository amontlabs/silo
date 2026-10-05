import { execFileSync, spawn } from "node:child_process"
import { createHash } from "node:crypto"
import { createReadStream, createWriteStream, existsSync, readFileSync, realpathSync } from "node:fs"
import { mkdir, mkdtemp, rename, rm, stat, writeFile } from "node:fs/promises"
import { dirname, resolve } from "node:path"
import { pipeline } from "node:stream/promises"
import { fileURLToPath } from "node:url"
import { createGzip } from "node:zlib"

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..")
/** The pinned LCU release archive for an image architecture, from the one lock that pins it. */
export function lcuArchive(architecture) {
  const lock = JSON.parse(readFileSync(resolve(root, "src-tauri/guest/lcu-lock.json"), "utf8"))
  const asset = lock.assets?.[architecture]
  if (lock.schemaVersion !== 1 || !/^[0-9][0-9A-Za-z.]*$/.test(lock.version) || !asset
    || !/^[0-9a-f]{64}$/.test(asset.sha256)
    || !asset.url.startsWith(`https://github.com/amontlabs/lcu/releases/download/v${lock.version}/`)) {
    throw new Error("The LCU lock is invalid")
  }
  return { version: lock.version, url: asset.url, sha256: asset.sha256, name: asset.url.split("/").pop() }
}

export function verifyGuestImage(architecture, imageReference, { run = execFileSync } = {}) {
  if (!["arm64", "amd64"].includes(architecture)) throw new Error("Unsupported guest image architecture")
  const tools = readFileSync(resolve(root, "src-tauri/guest/verify-tools.sh"), "utf8")
  const lcu = lcuArchive(architecture)
  const check = `${tools}
curl --version
curl -fsS file:///etc/os-release -o /dev/null
command -v sudo >/dev/null || { echo "Guest image is missing sudo" >&2; exit 1; }
python3 -c 'import json; assert json.loads("true") is True'
test -x /usr/lib/openssh/sftp-server
test -x /usr/bin/selkies || { echo "Guest image is missing the Selkies streamer" >&2; exit 1; }
test -x /usr/bin/xfce4-session || { echo "Guest image is missing the Xfce session" >&2; exit 1; }
python3 -c 'import json; m = json.load(open("/usr/local/share/silo/guest-image.json")); assert m["schemaVersion"] == 1 and m["version"] == "'${GUEST_IMAGE_VERSION}'" and {"desktop", "accessibility"} <= set(m["capabilities"]) and m["streamerVersion"] == "2.0.0"'
python3 -c 'import json; m = json.load(open("/usr/local/share/silo/guest-image.json")); assert "lcu-archive" in m["capabilities"]'
# The pinned LCU archive (guest/lcu-lock.json) is staged unextracted; LCU itself and any ChatGPT app are not in the image.
echo '${lcu.sha256}  /usr/local/share/silo/lcu/${lcu.name}' | sha256sum --check --status || { echo "The staged LCU archive does not match guest/lcu-lock.json" >&2; exit 1; }
test "$(ls /usr/local/share/silo/lcu | wc -l)" = 1 || { echo "Only the pinned LCU archive may be staged" >&2; exit 1; }
test ! -e /opt/lcu && test ! -e /usr/lib/chatgpt && test ! -e /opt/silo || { echo "The image must not contain LCU or a ChatGPT app" >&2; exit 1; }
test -s /usr/share/doc/silo-guest-third-party/NOTICES.md && grep -q 'x264' /usr/share/doc/silo-guest-third-party/NOTICES.md || { echo "The third-party notices are missing from the image" >&2; exit 1; }
test -x /usr/local/libexec/silo-accessibility
python3 -c 'import py_compile; py_compile.compile("/usr/local/libexec/silo-accessibility", cfile="/tmp/silo-accessibility.pyc", doraise=True)' || { echo "The accessibility poller does not compile" >&2; exit 1; }
test -f /etc/xdg/autostart/silo-accessibility.desktop
autostart_exec=$(sed -n 's/^Exec=//p' /etc/xdg/autostart/silo-accessibility.desktop | head -n 1 | cut -d' ' -f1)
test -n "$autostart_exec" && test -x "$autostart_exec" || { echo "The accessibility autostart entry does not name an executable" >&2; exit 1; }
leftovers=$(find / -xdev \\( -name '*.deb' -o -name 'selkies*.tar*' \\) -not -path '/proc/*' 2>/dev/null; find /var/cache/apt/archives /var/lib/apt/lists -type f 2>/dev/null)
test -z "$leftovers" || { echo "Guest image keeps package files: $leftovers" >&2; exit 1; }
# A disposable account in a fresh session bus must see accessibility enabled by the image defaults alone.
useradd --create-home --shell /bin/sh silo-verify-a11y
trap 'userdel --remove silo-verify-a11y >/dev/null 2>&1 || true' EXIT
test "$(sudo -n -u silo-verify-a11y -H dbus-launch --exit-with-session gsettings get org.gnome.desktop.interface toolkit-accessibility)" = true || { echo "toolkit-accessibility is not enabled by default for new accounts" >&2; exit 1; }
grep -qx 'toolkit-accessibility=true' /etc/dconf/db/local.d/00-silo-accessibility
test -f /etc/dconf/db/local && test "$(xdg-mime query default text/plain)" = org.gnome.TextEditor.desktop
if dpkg -s mousepad >/dev/null 2>&1; then echo "Mousepad crashes under AT-SPI paste and must not be installed" >&2; exit 1; fi
if getent passwd silo || getent group silo; then
  echo "The working account must be provisioned per VM, not preinstalled in the image" >&2
  exit 1
fi
sudo -n -u nobody sh -ec 'test "$(id -u)" != 0; /usr/lib/openssh/sftp-server -Q requests >/dev/null'
`
  run("docker", ["run", "--rm", "--pull", "never", "--network", "none", "--platform", `linux/${architecture}`, imageReference, "sh", "-ec", check], { stdio: "inherit" })
}

/** The recipe version. Increment it for every image update; published versions are never reused. */
export const GUEST_IMAGE_VERSION = "ubuntu-24.04-v4"

/**
 * Names derived from the recipe version and the publishing repository. The
 * publication workflow reads these instead of repeating the version or owner.
 */
export function guestImageMetadata(env = process.env) {
  const repository = env.GITHUB_REPOSITORY || env.GH_REPO
    || /github\.com\/([^/]+\/[^/.]+)/.exec(JSON.parse(readFileSync(resolve(root, "package.json"), "utf8")).repository.url)?.[1]
  const owner = repository?.split("/")[0]
  if (!owner || !/^[A-Za-z0-9-]+$/.test(owner)) throw new Error("Cannot determine the publishing GitHub owner.")
  const match = /^ubuntu-(\d+\.\d+)-(v\d+)$/.exec(GUEST_IMAGE_VERSION)
  if (!match) throw new Error("Guest image versions look like ubuntu-24.04-v4.")
  return {
    version: GUEST_IMAGE_VERSION,
    // Container registries require lowercase repository names.
    image: `ghcr.io/${owner.toLowerCase()}/silo-guest:${GUEST_IMAGE_VERSION}`,
    tag: `guest-${GUEST_IMAGE_VERSION}`,
    title: `Silo guest Ubuntu ${match[1]} ${match[2]}`,
  }
}

export async function buildGuestImage(architecture) {
  if (!["arm64", "amd64"].includes(architecture)) throw new Error("Usage: node scripts/build-guest-image.mjs arm64|amd64|metadata")
  const { version, image: imageName } = guestImageMetadata()
  const imageReference = `${imageName}-${architecture}`
  const revision = process.env.GITHUB_SHA || execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim()
  const output = resolve(root, "src-tauri/guest-image-artifacts", architecture)
  await mkdir(output, { recursive: true })
  execFileSync("docker", ["build", "--label", `org.opencontainers.image.revision=${revision}`, "--platform", `linux/${architecture}`, "-f", resolve(root, "guest-image/Dockerfile"), "-t", imageReference, root], { stdio: "inherit" })
  const image = JSON.parse(execFileSync("docker", ["image", "inspect", imageReference], { encoding: "utf8" }))[0]
  verifyGuestImage(architecture, imageReference)
  const packages = execFileSync("docker", ["run", "--rm", "--network", "none", "--platform", `linux/${architecture}`, imageReference, "cat", "/usr/local/share/silo-packages.txt"], { encoding: "utf8" })
  const stage = await mkdtemp(resolve(output, ".export-"))
  const archive = resolve(stage, "image.tar.gz")
  try {
    let unpackedBytes = 0
    const save = spawn("docker", ["image", "save", imageReference], { stdio: ["ignore", "pipe", "inherit"] })
    const exited = new Promise((resolve, reject) => { save.on("error", reject); save.on("exit", code => code === 0 ? resolve() : reject(new Error(`docker save exited ${code}`))) })
    save.stdout.on("data", chunk => { unpackedBytes += chunk.length })
    const outcomes = await Promise.allSettled([
      pipeline(save.stdout, createGzip({ level: 9 }), createWriteStream(archive)).catch(error => {
        save.kill("SIGTERM")
        throw error
      }), exited,
    ])
    for (const outcome of outcomes) if (outcome.status === "rejected") throw outcome.reason
    const hash = createHash("sha256")
    for await (const chunk of createReadStream(archive)) hash.update(chunk)
    const manifest = { schemaVersion: 1, version, ubuntuVersion: "24.04", architecture: architecture === "arm64" ? "aarch64" : "x86_64", imageReference, imageDigest: image.Id, archiveSha256: hash.digest("hex"), archiveBytes: (await stat(archive)).size, unpackedBytes, baseImage: "ubuntu:24.04@sha256:224a1869083a311ef3f13648a154ba79832fbef6364d31493642ca03082da254", packages: Object.fromEntries(packages.trim().split("\n").map(line => line.split("\t"))) }
    await writeFile(resolve(stage, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`)
    await rename(archive, resolve(output, "image.tar.gz"))
    await rename(resolve(stage, "manifest.json"), resolve(output, "manifest.json"))
    console.log(`Built ${imageReference}: ${manifest.archiveBytes} compressed bytes, config ${manifest.imageDigest}`)
  } finally {
    await rm(stage, { recursive: true, force: true })
  }
}

if (process.argv[1] && existsSync(process.argv[1]) && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[2] === "metadata") {
    // key=value lines for $GITHUB_OUTPUT.
    for (const [key, value] of Object.entries(guestImageMetadata())) console.log(`${key}=${value}`)
  } else {
    await buildGuestImage(process.argv[2])
  }
}
