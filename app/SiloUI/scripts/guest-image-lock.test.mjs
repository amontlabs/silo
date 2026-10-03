import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"
import { dirname, join } from "node:path"
import test from "node:test"
import { fileURLToPath } from "node:url"

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..")
const read = path => readFile(join(appRoot, path), "utf8")
const architectures = { arm64: "aarch64", amd64: "x86_64" }

test("the guest image lock pins one checksummed, sized archive per architecture", async () => {
  const lock = JSON.parse(await read("guest-image/image-lock.json"))
  assert.match(lock.releaseUrl, /^https:\/\/github\.com\/amontlabs\/silo\/releases\/download\/guest-ubuntu-[0-9.]+-v\d+$/)
  for (const [key, architecture] of Object.entries(architectures)) {
    const image = lock.images[key]
    assert.equal(image.schemaVersion, 1)
    assert.equal(image.architecture, architecture)
    assert.equal(lock.releaseUrl.endsWith(`guest-${image.version}`), true, key)
    assert.match(image.archiveSha256, /^[0-9a-f]{64}$/)
    assert.match(image.imageDigest, /^sha256:[0-9a-f]{64}$/)
    assert.match(image.imageReference, new RegExp(`:${image.version}-${key}$`))
    assert.ok(Number.isSafeInteger(image.archiveBytes) && image.archiveBytes > 0)
    assert.ok(Number.isSafeInteger(image.unpackedBytes) && image.unpackedBytes > image.archiveBytes)
  }
})

test("installers do not bundle the guest image and runtime preparation does not stage it", async () => {
  const config = JSON.parse(await read("src-tauri/tauri.conf.json"))
  for (const [source, destination] of Object.entries(config.bundle.resources)) {
    assert.doesNotMatch(`${source} ${destination}`, /guest-image/)
  }
  const prepare = await read("scripts/prepare-microsandbox-runtime.mjs")
  assert.doesNotMatch(prepare, /guest/i)
})

test("the app embeds the lock it downloads the image from", async () => {
  const source = await read("src-tauri/src/guest_image.rs")
  assert.match(source, /include_str!\("\.\.\/\.\.\/guest-image\/image-lock\.json"\)/)
})
