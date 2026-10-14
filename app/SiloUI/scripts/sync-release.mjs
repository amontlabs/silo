import { execFileSync } from "node:child_process"
import { readFileSync, writeFileSync, existsSync, realpathSync, mkdirSync, mkdtempSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"

const app = resolve(dirname(fileURLToPath(import.meta.url)), "..")

/** Silo stays below 1.0.0 until the owner explicitly decides on a stable release. */
export function assertPreStable(version, allowStable = false) {
  if (!allowStable && Number(version.split(".")[0]) >= 1) {
    throw new Error(`Silo releases stay below 1.0.0; refusing ${version}. Use a minor changeset for breaking changes, or pass --allow-stable only for an approved 1.0 release.`)
  }
}

function checkVersion(version, allowStable) {
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version) || version === "0.0.0") {
    throw new Error("Desktop releases require a nonzero stable version.")
  }
  assertPreStable(version, allowStable)
}

const notesFile = (root, version) => resolve(root, "../../docs/releases", `${version}.md`)

/** Compute the npm lock and Rust metadata for version without writing anything. */
function mirroredMetadata(root, version) {
  const read = path => readFileSync(resolve(root, path), "utf8")
  const lock = JSON.parse(read("package-lock.json"))
  if (!lock.packages?.[""] || lock.name !== "silo-ui") throw new Error("Unexpected npm lockfile structure.")
  lock.version = version
  lock.packages[""].version = version
  // Update only Silo's package stanza, preserving every dependency and checksum.
  const replaceVersion = (text, section) => {
    let count = 0
    const updated = text.replace(section, block => {
      if (!/^name = "silo-ui"$/m.test(block)) return block
      return block.replace(/^version = "[^"]+"$/m, () => { count++; return `version = "${version}"` })
    })
    if (count !== 1) throw new Error("Expected exactly one silo-ui version in Rust metadata.")
    return updated
  }
  return {
    lock,
    cargo: replaceVersion(read("src-tauri/Cargo.toml"), /^\[package\]\n[\s\S]*?(?=^\[|(?![\s\S]))/gm),
    cargoLock: replaceVersion(read("Cargo.lock"), /^\[\[package\]\]\n[\s\S]*?(?=^\[\[package\]\]|(?![\s\S]))/gm),
  }
}

/** Changesets owns the version and changelog; mirror them into desktop metadata. */
export function syncRelease(root = app, { allowStable = false } = {}) {
  const read = path => readFileSync(resolve(root, path), "utf8")
  const version = JSON.parse(read("package.json")).version
  checkVersion(version, allowStable)
  const changelog = read("CHANGELOG.md")
  const heading = `## ${version}`
  const lines = changelog.split("\n")
  const start = lines.indexOf(heading)
  if (start < 0) throw new Error(`No changelog for ${version}. Run npm run release:version first.`)
  const next = lines.findIndex((line, index) => index > start && /^## \d+\.\d+\.\d+(?:\s|$)/.test(line))
  const notes = lines.slice(start + 1, next < 0 ? undefined : next).join("\n").trim()
  if (!notes) throw new Error(`Release notes for ${version} are empty.`)

  const { lock, cargo, cargoLock } = mirroredMetadata(root, version)
  const notesPath = notesFile(root, version)
  const content = `# Silo ${version}\n\n${notes}\n`
  // A retry must preserve reviewed notes, never silently replace an existing release.
  if (existsSync(notesPath) && readFileSync(notesPath, "utf8") !== content) {
    throw new Error(`${notesPath} already exists with different notes. Review it before replacing it.`)
  }
  // Validate every input before writing any mirrored file.
  writeFileSync(resolve(root, "package-lock.json"), JSON.stringify(lock, null, 2) + "\n")
  writeFileSync(resolve(root, "src-tauri/Cargo.toml"), cargo)
  writeFileSync(resolve(root, "Cargo.lock"), cargoLock)
  mkdirSync(dirname(notesPath), { recursive: true })
  writeFileSync(notesPath, content)
  return version
}

/**
 * Run every synchronization check against the version Changesets plans, before
 * `changeset version` consumes a changeset or rewrites the changelog.
 */
export function precheckRelease(root, version, { allowStable = false } = {}) {
  checkVersion(version, allowStable)
  mirroredMetadata(root, version)
  const notesPath = notesFile(root, version)
  // Notes for a version that has not been prepared yet are stale leftovers.
  if (existsSync(notesPath)) {
    throw new Error(`${notesPath} already exists before ${version} was prepared. Remove the stale notes or review them first.`)
  }
}

/** npm run release:version: plan, precheck, then version with Changesets and synchronize. */
export function versionRelease(root = app, { allowStable = false, run = (command, args) => execFileSync(command, args, { cwd: root, stdio: "inherit" }) } = {}) {
  const changesets = resolve(root, "node_modules/@changesets/cli/bin.js")
  const directory = mkdtempSync(join(tmpdir(), "silo-release-plan-"))
  let plan
  try {
    const output = join(directory, "plan.json")
    run(process.execPath, [changesets, "status", "--output", output])
    plan = JSON.parse(readFileSync(output, "utf8"))
  } finally {
    rmSync(directory, { recursive: true, force: true })
  }
  const release = plan.releases?.find(entry => entry.name === "silo-ui")
  if (!release) throw new Error("No pending silo-ui changesets. Add one with npm run changeset first.")
  precheckRelease(root, release.newVersion, { allowStable })
  run(process.execPath, [changesets, "version"])
  return syncRelease(root, { allowStable })
}

if (process.argv[1] && existsSync(process.argv[1]) && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const options = { allowStable: process.argv.includes("--allow-stable") }
    const version = process.argv[2] === "version" ? versionRelease(app, options) : syncRelease(app, options)
    console.log(`Prepared Silo ${version}. Review and commit the version files, changelog, release notes, and consumed changesets. Then run npm run release:draft.`)
  } catch (error) {
    console.error(error.message)
    process.exitCode = 1
  }
}
